import { writable, type Readable } from 'svelte/store';
import type {
	SpaceEvent,
	TaskFinished,
	TaskStarted,
	TaskStep,
	Timite
} from '../../gen/tim/api/g1/api_pb';
import type { Timestamp } from '@bufbuild/protobuf/wkt';
import type { TimClient } from './client';
import type { ChannelPhase, TimConnect, TimSpaceHandler } from './connect';
import type { TimStorage } from './storage';
import type { WorklogItem } from './worklog';

const HISTORY_PAGE_SIZE = 50;

/** A task another timite reported and has not finished yet. */
type LiveTask = { timiteId: bigint; task: string; step?: string };

export class TimSpace implements TimSpaceHandler {
	private localId = 0n;
	private started = false;
	private timites = new Map<bigint, string>();
	private readonly connectedTimite = writable<string | null>(null);
	readonly timite: Readable<string | null> = this.connectedTimite;
	private readonly typing = new Set<bigint>();
	/** Running tasks by task id, in start order. */
	private readonly tasks = new Map<bigint, LiveTask>();
	private readonly statusLabels = writable<string[]>([]);
	/** What other timites are doing right now, as "username typing" or "username working: step". */
	readonly status: Readable<string[]> = this.statusLabels;
	/** Whether the space was told that this timite is typing. */
	private typingSent = false;

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
		await this.setTyping(false);
		await this.client.sendMessage(content);
	}

	/** Tells the space when this timite starts or stops typing. */
	async setTyping(typing: boolean) {
		if (typing === this.typingSent) return;
		this.typingSent = typing;
		try {
			if (typing) await this.client.sendTypingStarted();
			else await this.client.sendTypingStopped();
		} catch (error) {
			console.warn('TimSpace: typing report failed', error);
		}
	}

	onSpaceUpdate(update: SpaceEvent) {
		this.trackStatus(update);
		const item = this.asMessageItem(update);
		if (!item) return;
		this.append(item);
	}

	/** Keeps the typing set and the running tasks in step with the event stream. */
	private trackStatus(update: SpaceEvent) {
		const payload = update.data;
		const me = this.client.getTimite()?.id;
		switch (payload.case) {
			case 'eventTypingStarted': {
				const id = payload.value.typingStarted?.timiteId;
				if (id !== undefined && id !== me) this.typing.add(id);
				break;
			}
			case 'eventTypingStopped': {
				const id = payload.value.typingStopped?.timiteId;
				if (id !== undefined) this.typing.delete(id);
				break;
			}
			case 'eventNewMessage': {
				const id = payload.value.message?.senderId;
				if (id !== undefined) this.typing.delete(id);
				break;
			}
			case 'eventTaskStarted':
				this.taskStarted(payload.value.taskStarted);
				break;
			case 'eventTaskStep':
				this.taskStep(payload.value.taskStep);
				break;
			case 'eventTaskFinished':
				this.taskFinished(payload.value.taskFinished);
				break;
			case 'eventTimiteDisconnected': {
				const id = payload.value.timite?.id;
				if (id === undefined) break;
				this.typing.delete(id);
				for (const [taskId, task] of this.tasks) {
					if (task.timiteId === id) this.tasks.delete(taskId);
				}
				break;
			}
			default:
				return;
		}
		this.refreshStatus();
	}

	private taskStarted(started?: TaskStarted) {
		if (!started) return;
		this.tasks.set(started.taskId, { timiteId: started.timiteId, task: started.task });
	}

	private taskStep(step?: TaskStep) {
		const task = step && this.tasks.get(step.taskId);
		if (task) task.step = step.step;
	}

	private taskFinished(finished?: TaskFinished) {
		if (finished) this.tasks.delete(finished.taskId);
	}

	private refreshStatus() {
		const typing = [...this.typing].map((id) => `${this.formatAuthor(id)} typing`).sort();
		const working = [...this.tasks.values()].map(
			(task) => `${this.formatAuthor(task.timiteId)} working: ${task.step ?? task.task}`
		);
		this.statusLabels.set([...typing, ...working]);
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
			case 'eventTaskStarted':
				return this.buildTaskItem(update, payload.value.taskStarted, 'started');
			case 'eventTaskFinished': {
				const finished = payload.value.taskFinished;
				if (!finished) return null;
				const task = this.tasks.get(finished.taskId)?.task ?? `task-${finished.taskId}`;
				return this.buildTaskItem(update, { timiteId: finished.timiteId, task }, 'finished');
			}
			default:
				return null;
		}
	}

	private buildTaskItem(
		update: SpaceEvent,
		task: { timiteId: bigint; task: string } | undefined,
		action: 'started' | 'finished'
	): WorklogItem | null {
		if (!task) return null;
		return {
			kind: 'sysmsg',
			id: update.metadata?.id ?? this.nextLocalId(),
			author: 'system',
			content: `${this.formatAuthor(task.timiteId)} ${action}: ${task.task}`,
			time: this.formatTime(update.metadata?.emittedAt)
		};
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
		return this.formatTimite(senderId, this.timites.get(senderId));
	}

	private formatTimite(id: bigint, username?: string): string {
		const label = (username ?? '').trim() || 'timite';
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
		const known = this.timites.get(timite.id) ?? timite.username;
		this.connectedTimite.set(this.formatTimite(timite.id, known));
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
			content: `${this.formatTimite(timite.id, timite.username)} ${action}.`,
			time: this.formatTime(update.metadata?.emittedAt)
		};
	}

	private rememberTimite(timite: Timite) {
		const username = timite.username.trim();
		if (!username) return;
		this.timites.set(timite.id, username);
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
