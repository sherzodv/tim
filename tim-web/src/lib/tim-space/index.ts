import type { SpaceUpdate } from '../../gen/tim/api/g1/api_pb';
import type { TimClient } from '../tim-client';
import type { ChannelPhase, TimConnect, TimSpaceHandler } from '../tim-connect';
import type { TimSpaceStorage } from './storage';
import type { WorkLogItem } from '../ui/work-log/types';

export class TimSpace implements TimSpaceHandler {
	private localId = 0n;
	private started = false;

	constructor(
		private readonly client: TimClient,
		private readonly connector: TimConnect,
		private readonly storage: TimSpaceStorage
	) {}

	start() {
		if (this.started) return;
		this.started = true;
		void this.connector.start(this);
	}

	stop() {
		if (!this.started) return;
		this.started = false;
		this.connector.stop();
	}

	async send(content: string) {
		await this.client.sendMessage(content);
	}

	onSpaceUpdate(update: SpaceUpdate) {
		if (update.event?.case !== 'spaceNewMessage') return;
		const message = update.event.value?.message;
		if (!message) return;
		this.append({
			id: message.id ?? this.nextLocalId(),
			kind: 'msg',
			author: this.formatAuthor(message.senderId),
			content: message.content ?? ''
		});
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

	private append(item: WorkLogItem) {
		this.storage.append(item);
	}

	private nextLocalId(): bigint {
		this.localId += 1n;
		return this.localId;
	}

	private formatAuthor(senderId?: bigint): string {
		return senderId !== undefined ? `timite#${senderId}` : 'unknown';
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
