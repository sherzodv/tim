import type { SpaceUpdate } from '../../gen/tim/api/g1/api_pb';
import {
	type TimClient
} from '../tim-client';

export type ChannelPhase = 'idle' | 'connecting' | 'open' | 'reconnecting' | 'stopped';

export type TimSpaceHandler = {
	onSpaceUpdate(update: SpaceUpdate): void;
	onPhaseChange?(phase: ChannelPhase): void;
};

export type TimConnectOptions = {
	receiveOwnMessages?: boolean;
};

export interface TimConnect {
	start(handler: TimSpaceHandler): Promise<void>;
	stop(): void;
}

class TimConnectImpl implements TimConnect {
	private readonly client: TimClient;
	private readonly receiveOwnMessages: boolean;

	private phase: ChannelPhase = 'idle';
	private streamAbort: AbortController | null = null;
	private runner: Promise<void> | null = null;

	constructor(client: TimClient, options: TimConnectOptions = {}) {
		this.client = client;
		this.receiveOwnMessages = options.receiveOwnMessages ?? true;
	}

	async start(handler: TimSpaceHandler): Promise<void> {
		if (this.runner) {
			return;
		}
		this.streamAbort = new AbortController();
		const { signal } = this.streamAbort;
		const runner = this.run(handler, signal);
		this.runner = runner
			.catch((error) => {
				if (!isAbortError(error)) {
					console.error('TimConnect: fatal stream error', error);
				}
			})
			.finally(() => {
				this.runner = null;
				this.streamAbort = null;
				this.setPhase('stopped', handler);
			});
		return Promise.resolve();
	}

	stop() {
		if (this.streamAbort) {
			this.streamAbort.abort();
			this.streamAbort = null;
		}
		this.runner = null;
		this.setPhase('stopped');
	}

	private async run(handler: TimSpaceHandler, signal: AbortSignal) {
		this.setPhase('connecting', handler);
		const stream = await this.client.subscribeToSpace(
			this.receiveOwnMessages,
			signal
		);
		this.setPhase('open', handler);

		try {
			for await (const update of stream) {
				handler.onSpaceUpdate(update);
			}
		} catch (error) {
			if (!isAbortError(error)) {
				console.error('TimConnect: stream error', error);
				this.setPhase('reconnecting', handler);
			}
		}
	}

	private setPhase(phase: ChannelPhase, handler?: TimSpaceHandler) {
		if (this.phase === phase) return;
		this.phase = phase;
		handler?.onPhaseChange?.(phase);
	}
}

export const createTimConnect = (client: TimClient, options?: TimConnectOptions): TimConnect =>
	new TimConnectImpl(client, options);

function isAbortError(error: unknown): boolean {
	if (!error || typeof error !== 'object') return false;
	return (error as { name?: string }).name === 'AbortError';
}
