import { writable, type Readable } from 'svelte/store';
import type { Activity, SpaceEvent, Timite } from '../../gen/tim/api/g1/api_pb';
import type { Timestamp } from '@bufbuild/protobuf/wkt';
import type { TimClient } from './client';
import type { ChannelPhase, TimConnect, TimSpaceHandler } from './connect';
import type { TimStorage } from './storage';
import type { WorklogItem } from './worklog';

const HISTORY_PAGE_SIZE = 50;
/** How long a received activity such as "typing" stays visible. */
const ACTIVITY_TTL_MS = 5000;
/** Minimal pause between own "typing" activities. */
const TYPING_INTERVAL_MS = 3000;

type LiveActivity = { label: string; until: number };

export class TimSpace implements TimSpaceHandler {
	private localId = 0n;
	private started = false;
	private timites = new Map<bigint, string>();
	private readonly connectedTimite = writable<string | null>(null);
	readonly timite: Readable<string | null> = this.connectedTimite;
	private readonly liveActivities = new Map<bigint, LiveActivity>();
	private readonly activityLabels = writable<string[]>([]);
	/** Activities of other timites still in effect, as "nick kind: payload". */
	readonly activities: Readable<string[]> = this.activityLabels;
	private lastTyping = 0;

	constructor(
		private readonly client: TimClient,
		private readonly connector: TimConnect,
		private readonly storage: TimStorage
	) {}

	start() {
		if (this.started) return;
		this.started = true;
		this.setConnectedTimite(this.client.getTimite());
		void this.bootstrap();
	}

	stop() {
		if (!this.started) return;
		this.started = false;
		this.connector.stop();
	}

	private async bootstrap() {
		const lastEventId = await this.loadHistory();
		if (!this.started) return;
		await this.connector.start(this, lastEventId);
	}

	/** Returns the id of the newest loaded event, so the live stream can continue after it. */
	private async loadHistory(): Promise<bigint | undefined> {
		try {
			const timeline = await this.client.getTimeline(0n, HISTORY_PAGE_SIZE);
			this.captureTimites(timeline.timites);
			const history = this.buildHistoryItems(timeline.events);
			this.storage.set(history);
			this.setConnectedTimite(this.client.getTimite());
			return maxEventId(timeline.events);
		} catch (error) {
			console.error('TimSpace: failed to load history', error);
			return undefined;
		}
	}

	async send(content: string) {
		await this.client.sendMessage(content);
		this.lastTyping = 0;
	}

	/** Tells the space the user is composing, at most once per interval. */
	notifyTyping() {
		const now = Date.now();
		if (now - this.lastTyping < TYPING_INTERVAL_MS) return;
		this.lastTyping = now;
		this.client.sendActivity('typing').catch((error) => {
			console.warn('TimSpace: typing activity failed', error);
		});
	}

	onSpaceUpdate(update: SpaceEvent) {
		if (update.data.case === 'eventActivity') {
			const activity = update.data.value.activity;
			if (activity) this.activityReceived(activity);
			return;
		}
		const item = this.asMessageItem(update);
		if (!item) return;
		this.append(item);
	}

	private activityReceived(activity: Activity) {
		if (activity.timiteId === this.client.getTimite()?.id) return;
		const who = this.formatAuthor(activity.timiteId);
		const label = activity.payload
			? `${who} ${activity.kind}: ${activity.payload}`
			: `${who} ${activity.kind}`;
		this.liveActivities.set(activity.timiteId, { label, until: Date.now() + ACTIVITY_TTL_MS });
		this.refreshActivities();
		setTimeout(() => this.refreshActivities(), ACTIVITY_TTL_MS + 50);
	}

	private refreshActivities() {
		const now = Date.now();
		for (const [id, activity] of this.liveActivities) {
			if (activity.until <= now) this.liveActivities.delete(id);
		}
		const labels = [...this.liveActivities.values()].map((activity) => activity.label).sort();
		this.activityLabels.set(labels);
	}

	onPhaseChange(phase: ChannelPhase) {
		const description = this.describePhase(phase);
		if (!description) return;
		this.append({
			kind: 'sysmsg',
			id: this.nextLocalId(),
			author: 'system',
			content: description
		});
	}

	private append(item: WorklogItem) {
		this.storage.append(item);
	}

	private buildHistoryItems(events: SpaceEvent[]): WorklogItem[] {
		const sorted = [...events].sort((left, right) => {
			const leftId = left.metadata?.id ?? 0n;
			const rightId = right.metadata?.id ?? 0n;
			if (leftId === rightId) return 0;
			return leftId < rightId ? -1 : 1;
		});
		return sorted
			.map((event) => this.asMessageItem(event))
			.filter((item): item is WorklogItem => item !== null);
	}

	private asMessageItem(update: SpaceEvent): WorklogItem | null {
		const payload = update.data;
		if (!payload) return null;
		switch (payload.case) {
			case 'eventNewMessage': {
				const message = payload.value?.message;
				if (!message) return null;
				return {
					kind: 'msg',
					id: update.metadata?.id ?? this.nextLocalId(),
					author: this.formatAuthor(message.senderId),
					content: message.content ?? '',
					time: this.formatTime(update.metadata?.emittedAt)
				};
			}
			case 'eventTimiteConnected':
				return this.buildPresenceItem(update, payload.value?.timite, 'connected');
			case 'eventTimiteDisconnected':
				return this.buildPresenceItem(update, payload.value?.timite, 'disconnected');
			default:
				return null;
		}
	}

	private captureTimites(timites: Timite[]) {
		this.timites.clear();
		for (const timite of timites) {
			this.rememberTimite(timite);
		}
		this.setConnectedTimite(this.client.getTimite());
	}

	private nextLocalId(): bigint {
		this.localId += 1n;
		return this.localId;
	}

	private formatAuthor(senderId?: bigint): string {
		if (senderId === undefined) return 'unknown';
		const nick = this.timites.get(senderId);
		return this.formatTimite(senderId, nick);
	}

	private formatTimite(id: bigint, nick?: string): string {
		const preferredNick = (nick ?? '').trim();
		const label = preferredNick || 'timite';
		return `${label}#${id}`;
	}

	private formatTime(timestamp?: Timestamp): string | undefined {
		if (!timestamp) return undefined;
		const millis =
			Number(timestamp.seconds) * 1000 + Math.floor((timestamp.nanos ?? 0) / 1_000_000);
		const date = new Date(millis);
		const year = date.getFullYear();
		const month = `${date.getMonth() + 1}`.padStart(2, '0');
		const day = `${date.getDate()}`.padStart(2, '0');
		const hours = `${date.getHours()}`.padStart(2, '0');
		const minutes = `${date.getMinutes()}`.padStart(2, '0');
		return `${year}-${month}-${day} ${hours}:${minutes}`;
	}

	private describePhase(phase: ChannelPhase): string | null {
		switch (phase) {
			case 'connecting':
				return 'Connecting to Tim space...';
			case 'open':
				return 'Connected to Tim space.';
			case 'reconnecting':
				return 'Connection lost, retrying...';
			case 'stopped':
				return 'Connection stopped.';
			default:
				return null;
		}
	}

	private setConnectedTimite(timite: Timite | null) {
		if (!timite) {
			this.connectedTimite.set(null);
			return;
		}
		this.rememberTimite(timite);
		const knownNick = this.timites.get(timite.id) ?? timite.nick;
		this.connectedTimite.set(this.formatTimite(timite.id, knownNick));
	}

	private buildPresenceItem(
		update: SpaceEvent,
		timite: Timite | undefined,
		action: 'connected' | 'disconnected'
	): WorklogItem | null {
		if (!timite) return null;
		this.rememberTimite(timite);
		return {
			kind: 'sysmsg',
			id: update.metadata?.id ?? this.nextLocalId(),
			author: 'system',
			content: `${this.formatTimite(timite.id, timite.nick)} ${action}.`,
			time: this.formatTime(update.metadata?.emittedAt)
		};
	}

	private rememberTimite(timite: Timite) {
		const nick = timite.nick.trim();
		if (!nick) return;
		this.timites.set(timite.id, nick);
	}
}

export const createTimSpace = (client: TimClient, connector: TimConnect, storage: TimStorage) =>
	new TimSpace(client, connector, storage);

export type { TimStorage } from './storage';

function maxEventId(events: SpaceEvent[]): bigint | undefined {
	let max: bigint | undefined;
	for (const event of events) {
		const id = event.metadata?.id;
		if (id !== undefined && (max === undefined || id > max)) max = id;
	}
	return max;
}
