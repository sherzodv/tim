import type { SpaceUpdate } from '../../gen/tim/api/g1/api_pb';
import type { ChannelPhase, TimSpaceHandler } from '../tim-connect';
import type { TimClient } from '../tim-client';

export class TimSpace implements TimSpaceHandler {
	constructor(private readonly client: TimClient) {
		console.log('[TimSpace] ready');
	}

	onSpaceUpdate(update: SpaceUpdate) {
		console.log('[TimSpace] update', update);
	}

	onPhaseChange(phase: ChannelPhase) {
		console.log('[TimSpace] phase ->', phase);
	}

	async send(content: string) {
		await this.client.sendMessage(content);
	}
}

export const createTimSpace = (client: TimClient) => new TimSpace(client);
