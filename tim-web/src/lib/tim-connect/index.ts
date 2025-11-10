import { ConnectError, Code } from '@connectrpc/connect';
import type { SpaceUpdate } from '../../gen/tim/api/g1/api_pb';
import { type TimClient } from '../tim-client';

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
	private loopAbort: AbortController | null = null;
	private runner: Promise<void> | null = null;

	constructor(client: TimClient, options: TimConnectOptions = {}) {
		this.client = client;
		this.receiveOwnMessages = options.receiveOwnMessages ?? true;
	}

	async start(handler: TimSpaceHandler): Promise<void> {
		if (this.runner) {
			return;
		}
		this.loopAbort = new AbortController();
		const { signal } = this.loopAbort;
		const runner = this.run(handler, signal);
		this.runner = runner
			.catch((error) => {
				if (!isAbortError(error)) {
					console.error('TimConnect: fatal stream error', error);
				}
			})
			.finally(() => {
				this.runner = null;
				this.loopAbort = null;
				this.setPhase('stopped', handler);
			});
		return Promise.resolve();
	}

	stop() {
		if (this.loopAbort) {
			this.loopAbort.abort();
			this.loopAbort = null;
		}
		this.runner = null;
		this.setPhase('stopped');
	}

	private async run(handler: TimSpaceHandler, signal: AbortSignal) {
		let attempt = 0;
		while (!signal.aborted) {
			try {
				this.setPhase(attempt === 0 ? 'connecting' : 'reconnecting', handler);
				const stream = await this.client.subscribeToSpace(this.receiveOwnMessages, signal);
				this.setPhase('open', handler);
				attempt = 0;
				for await (const update of stream) {
					if (signal.aborted) break;
					handler.onSpaceUpdate(update);
				}
				if (signal.aborted) break;
				attempt += 1;
			} catch (error) {
				if (signal.aborted && isAbortError(error)) {
					break;
				}
				if (shouldResetSession(error)) {
					this.client.resetSession();
				}
				if (!isExpectedStreamError(error)) {
					console.error('TimConnect: stream error', error);
				}
				attempt = Math.min(attempt + 1, 5);
			}

			if (signal.aborted) {
				break;
			}

			this.setPhase('reconnecting', handler);
			const delay = Math.min(500 * Math.pow(2, attempt), 5000);
			try {
				await wait(delay, signal);
			} catch {
				break;
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

function shouldResetSession(error: unknown): boolean {
	return isUnauthenticated(error) || isMissingTrailer(error);
}

function isUnauthenticated(error: unknown): error is ConnectError {
	return error instanceof ConnectError && error.code === Code.Unauthenticated;
}

function isMissingTrailer(error: unknown): boolean {
	return (
		error instanceof ConnectError &&
		error.code === Code.Unknown &&
		error.message.toLowerCase().includes('missing trailer')
	);
}

function isExpectedStreamError(error: unknown): boolean {
	return isMissingTrailer(error);
}

function wait(ms: number, signal: AbortSignal): Promise<void> {
	return new Promise((resolve, reject) => {
		const timer = setTimeout(() => {
			cleanup();
			resolve();
		}, ms);
		const cleanup = () => {
			clearTimeout(timer);
			signal.removeEventListener('abort', onAbort);
		};
		const onAbort = () => {
			cleanup();
			reject(new DOMException('Aborted', 'AbortError'));
		};
		if (signal.aborted) {
			onAbort();
			return;
		}
		signal.addEventListener('abort', onAbort);
	});
}
