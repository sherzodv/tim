import { browser } from '$app/environment';
import { createClient, type Client } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';
import { ConnectError, Code } from '@connectrpc/connect';
import { create } from '@bufbuild/protobuf';
import {
	TimApi,
	AuthenticateReqSchema,
	ClientInfoSchema,
	SendMessageReqSchema,
	SubscribeToSpaceReqSchema,
	TimiteSchema,
	type AuthenticateReq,
	type SendMessageReq,
	type SubscribeToSpaceReq
} from '../../gen/tim/api/g1/api_pb';

const SESSION_HEADER = 'tim-session-id' as const;
const BASE_URL = import.meta.env.VITE_TIM_CODE_URL ?? 'http://127.0.0.1:8787';

export type TimClientConf = {
	timiteId: bigint;
	nick: string;
	platform: string;
};

export class TimClient {
	private readonly client: Client<typeof TimApi>;
	private readonly conf: TimClientConf;
	private sessionId: bigint | null = null;
	private sessionInit: Promise<bigint> | null = null;

	constructor(conf: TimClientConf) {
		this.conf = conf;
		this.client = createClient(
			TimApi,
			createGrpcWebTransport({
				baseUrl: BASE_URL
			})
		);
	}

	private async ensureSession(): Promise<bigint> {
		if (this.sessionId !== null) {
			return this.sessionId;
		}
		if (this.sessionInit) {
			return this.sessionInit;
		}
		this.sessionInit = this.acquireSession();
		try {
			this.sessionId = await this.sessionInit;
			return this.sessionId;
		} finally {
			this.sessionInit = null;
		}
	}

	private async acquireSession(): Promise<bigint> {
		const request = buildAuthenticateRequest(this.conf);
		const response = await this.client.authenticate(request);
		const sessionId = response.session?.id;
		if (sessionId === undefined) {
			throw new ConnectError('missing session id in authenticate response', Code.Internal);
		}
		return sessionId;
	}

	async sendMessage(content: string): Promise<void> {
		const trimmed = content.trim();
		if (!trimmed) return;

		const sessionId = await this.ensureSession();
		const request = buildSendMessageRequest(trimmed);
		await this.client.sendMessage(request, {
			headers: buildSessionHeaders(sessionId)
		});
	}

	async subscribeToSpace(
		receiveOwnMessages: boolean,
		signal: AbortSignal
	) {
		const sessionId = await this.ensureSession();
		const request = buildSubscribeRequest(receiveOwnMessages);
		return this.client.subscribeToSpace(request, {
			signal,
			headers: buildSessionHeaders(sessionId)
		});
	}

	resetSession() {
		this.sessionId = null;
		this.sessionInit = null;
	}
}

export const createTimClient = (conf: TimClientConf) => new TimClient(conf);

export const createWebTimClientConf = (): TimClientConf => {
	const platform =
		browser && typeof navigator !== 'undefined'
			? `web:${navigator.userAgent}`
			: 'web';
	return {
		timiteId: BigInt(Date.now()),
		nick: `web-${Math.random().toString(36).slice(2, 7)}`,
		platform
	};
};

function buildAuthenticateRequest(identity: TimClientConf): AuthenticateReq {
	const timite = create(TimiteSchema, {
		id: identity.timiteId,
		nick: identity.nick
	});
	const clientInfo = create(ClientInfoSchema, {
		platform: identity.platform
	});
	return create(AuthenticateReqSchema, {
		timite,
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
