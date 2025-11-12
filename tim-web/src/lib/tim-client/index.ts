import { createClient, type Client } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';
import { ConnectError, Code } from '@connectrpc/connect';
import { create } from '@bufbuild/protobuf';
import {
	TimGrpcApi,
	TrustedRegisterReqSchema,
	ClientInfoSchema,
	SendMessageReqSchema,
	SubscribeToSpaceReqSchema,
	type TrustedRegisterReq,
	type SendMessageReq,
	type SubscribeToSpaceReq
} from '../../gen/tim/api/g1/api_pb';

const SESSION_HEADER = 'tim-session-key' as const;
const BASE_URL = import.meta.env.VITE_TIM_CODE_URL ?? 'http://127.0.0.1:8787';

export type TimClientConf = {
	nick: string;
	platform: string;
};

export class TimClient {
	private readonly client: Client<typeof TimGrpcApi>;
	private readonly conf: TimClientConf;
	private sessionKey: string | null = null;
	private sessionInit: Promise<string> | null = null;

	constructor(conf: TimClientConf) {
		this.conf = conf;
		this.client = createClient(
			TimGrpcApi,
			createGrpcWebTransport({
				baseUrl: BASE_URL
			})
		);
	}

	private async ensureSession(): Promise<string> {
		if (this.sessionKey !== null) {
			return this.sessionKey;
		}
		if (this.sessionInit) {
			return this.sessionInit;
		}
		this.sessionInit = this.acquireSession();
		try {
			this.sessionKey = await this.sessionInit;
			return this.sessionKey;
		} finally {
			this.sessionInit = null;
		}
	}

	private async acquireSession(): Promise<string> {
		const request = buildTrustedRegisterRequest(this.conf);
		const response = await this.client.trustedRegister(request);
		const sessionKey = response.session?.key;
		if (sessionKey === undefined) {
			throw new ConnectError('missing session key in trusted register response', Code.Internal);
		}
		return sessionKey;
	}

	async sendMessage(content: string): Promise<void> {
		const trimmed = content.trim();
		if (!trimmed) return;

		const sessionKey = await this.ensureSession();
		const request = buildSendMessageRequest(trimmed);
		await this.client.sendMessage(request, {
			headers: buildSessionHeaders(sessionKey)
		});
	}

	async subscribeToSpace(receiveOwnMessages: boolean, signal: AbortSignal) {
		const sessionKey = await this.ensureSession();
		const request = buildSubscribeRequest(receiveOwnMessages);
		return this.client.subscribeToSpace(request, {
			signal,
			headers: buildSessionHeaders(sessionKey)
		});
	}

	resetSession() {
		this.sessionKey = null;
		this.sessionInit = null;
	}
}

export const createTimClient = (conf: TimClientConf) => new TimClient(conf);

function buildTrustedRegisterRequest(identity: TimClientConf): TrustedRegisterReq {
	const clientInfo = create(ClientInfoSchema, {
		platform: identity.platform
	});
	return create(TrustedRegisterReqSchema, {
		nick: identity.nick,
		clientInfo
	});
}

function buildSendMessageRequest(content: string): SendMessageReq {
	return create(SendMessageReqSchema, {
		content
	});
}

function buildSubscribeRequest(receiveOwnMessages: boolean): SubscribeToSpaceReq {
	return create(SubscribeToSpaceReqSchema, {
		receiveOwnMessages
	});
}

function buildSessionHeaders(sessionId: bigint | string): HeadersInit {
	return [[SESSION_HEADER, sessionId.toString()]];
}
