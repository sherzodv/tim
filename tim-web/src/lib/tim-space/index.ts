import type { SpaceEvent, Timite, Timestamp } from '../../gen/tim/api/g1/api_pb';
import type { TimClient } from '../tim-client';
import type { ChannelPhase, TimConnect, TimSpaceHandler } from '../tim-connect';
import type { TimSpaceStorage } from './storage';
import type { WorklogItem } from '../ui/worklog/types';

const HISTORY_PAGE_SIZE = 50;

export class TimSpace implements TimSpaceHandler {
	private localId = 0n;
	private started = false;
	private timites = new Map<bigint, string>();

	constructor(
		private readonly client: TimClient,
		private readonly connector: TimConnect,
		private readonly storage: TimSpaceStorage
	) {}

	start() {
		if (this.started) return;
		this.started = true;
		void this.bootstrap();
	}

	stop() {
		if (!this.started) return;
		this.started = false;
		this.connector.stop();
	}

	private async bootstrap() {
		await this.loadHistory();
		if (!this.started) return;
		await this.connector.start(this);
	}

	private async loadHistory() {
		try {
			const timeline = await this.client.getTimeline(0n, HISTORY_PAGE_SIZE);
			this.captureTimites(timeline.timites);
			const history = this.buildHistoryItems(timeline.events);
			this.storage.set(history);
		} catch (error) {
			console.error('TimSpace: failed to load history', error);
		}
	}

	async send(content: string) {
		await this.client.sendMessage(content);
	}

	onSpaceUpdate(update: SpaceEvent) {
		const item = this.asMessageItem(update);
		if (!item) return;
		this.append(item);
	}

	onPhaseChange(phase: ChannelPhase) {
		const description = this.describePhase(phase);
		if (!description) return;
		this.append({
			id: this.nextLocalId(),
			kind: 'sysmsg',
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
		if (update.data?.case !== 'eventNewMessage') return null;
		const message = update.data.value?.message;
		if (!message) return null;
		return {
			id: message.id ?? this.nextLocalId(),
			kind: 'msg',
			author: this.formatAuthor(message.senderId),
			content: message.content ?? '',
			time: this.formatTime(update.metadata?.emittedAt)
		};
	}

	private captureTimites(timites: Timite[]) {
		this.timites.clear();
		for (const timite of timites) {
			const nick = timite.nick.trim();
			if (!nick) continue;
			this.timites.set(timite.id, nick);
		}
	}

	private nextLocalId(): bigint {
		this.localId += 1n;
		return this.localId;
	}

	private formatAuthor(senderId?: bigint): string {
		if (senderId === undefined) return 'unknown';
		const nick = this.timites.get(senderId);
		return nick ?? `timite#${senderId}`;
	}

	private formatTime(timestamp?: Timestamp): string | undefined {
		if (!timestamp) return undefined;
		const millis = Number(timestamp.seconds) * 1000 + Math.floor((timestamp.nanos ?? 0) / 1_000_000);
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
}

export const createTimSpace = (
	client: TimClient,
	connector: TimConnect,
	storage: TimSpaceStorage
) => new TimSpace(client, connector, storage);

export type { TimSpaceStorage } from './storage';
