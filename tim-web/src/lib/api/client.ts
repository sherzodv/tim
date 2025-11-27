import { createClient, type Client } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';
import { ConnectError, Code } from '@connectrpc/connect';
import { create } from '@bufbuild/protobuf';
import {
	TimGrpcApi,
	TrustedRegisterReqSchema,
	TrustedConnectReqSchema,
	ClientInfoSchema,
	SendMessageReqSchema,
	SubscribeToSpaceReqSchema,
	GetTimelineReqSchema,
	TimiteSchema,
	type TrustedRegisterReq,
	type SendMessageReq,
	type SubscribeToSpaceReq,
	type GetTimelineRes
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
		// Try to load existing timite from localStorage
		const storedTimite = this.loadStoredTimite();

		if (storedTimite) {
			// Use trustedConnect with existing timite
			const connectReq = create(TrustedConnectReqSchema, {
				timite: storedTimite,
				clientInfo: create(ClientInfoSchema, {
					platform: this.conf.platform
				})
			});
			const response = await this.client.trustedConnect(connectReq);
			const sessionKey = response.session?.key;
			if (sessionKey === undefined) {
				throw new ConnectError('missing session key in trusted connect response', Code.Internal);
			}
			return sessionKey;
		} else {
			// Register new timite
			const request = buildTrustedRegisterRequest(this.conf);
			const response = await this.client.trustedRegister(request);
			const sessionKey = response.session?.key;
			const timiteId = response.session?.timiteId;
			if (sessionKey === undefined) {
				throw new ConnectError('missing session key in trusted register response', Code.Internal);
			}
			// Store timite for future sessions
			if (timiteId !== undefined) {
				this.storeTimite(timiteId, this.conf.nick);
			}
			return sessionKey;
		}
	}

	private loadStoredTimite() {
		if (typeof window === 'undefined') return null;
		try {
			const stored = localStorage.getItem('tim-timite');
			if (!stored) return null;
			const data = JSON.parse(stored);
			return create(TimiteSchema, {
				id: BigInt(data.id),
				nick: data.nick
			});
		} catch {
			return null;
		}
	}

	private storeTimite(id: bigint, nick: string) {
		if (typeof window === 'undefined') return;
		try {
			localStorage.setItem('tim-timite', JSON.stringify({ id: id.toString(), nick }));
		} catch (error) {
			console.warn('Failed to store timite in localStorage', error);
		}
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

	async getTimeline(offset: bigint = 0n, size = 50): Promise<GetTimelineRes> {
		const sessionKey = await this.ensureSession();
		const request = create(GetTimelineReqSchema, { offset, size });
		return this.client.getTimeline(request, {
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
