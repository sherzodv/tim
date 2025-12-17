import { createClient, type Client } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';
import { ConnectError, Code } from '@connectrpc/connect';
import { create } from '@bufbuild/protobuf';
import {
	ErrorCode,
	TimGrpcApi,
	TrustedRegisterReqSchema,
	TrustedConnectReqSchema,
	ClientInfoSchema,
	SendMessageReqSchema,
	SubscribeToSpaceReqSchema,
	GetTimelineReqSchema,
	TimiteSchema,
	type Timite,
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
	private timite: Timite | null = null;
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
			if (sessionKey !== undefined) {
				return sessionKey;
			}
			// If server explicitly says timite not found, clear and re-register.
			if (response.error === ErrorCode.TIMITE_NOT_FOUND) {
				this.clearStoredTimite();
				console.warn('Stored timite not found on server, re-registering');
				return this.registerTimite();
			}
			throw new ConnectError('missing session key in trusted connect response', Code.Internal);
		}

		// Register new timite
		return this.registerTimite();
	}

	private loadStoredTimite(): Timite | null {
		if (typeof window === 'undefined') return null;
		try {
			const stored = localStorage.getItem('tim-timite');
			if (!stored) return null;
			const data = JSON.parse(stored);
			const timite = create(TimiteSchema, {
				id: BigInt(data.id),
				nick: data.nick
			});
			this.timite = timite;
			return timite;
		} catch {
			return null;
		}
	}

	private storeTimite(timite: Timite) {
		if (typeof window === 'undefined') return;
		try {
			this.timite = timite;
			localStorage.setItem(
				'tim-timite',
				JSON.stringify({ id: timite.id.toString(), nick: timite.nick })
			);
		} catch (error) {
			console.warn('Failed to store timite in localStorage', error);
		}
	}

	private clearStoredTimite() {
		if (typeof window === 'undefined') return;
		try {
			this.timite = null;
			localStorage.removeItem('tim-timite');
		} catch (error) {
			console.warn('Failed to clear timite from localStorage', error);
		}
	}

	private async registerTimite(): Promise<string> {
		const request = buildTrustedRegisterRequest(this.conf);
		const response = await this.client.trustedRegister(request);
		const sessionKey = response.session?.key;
		const timiteId = response.session?.timiteId;
		if (sessionKey === undefined) {
			throw new ConnectError('missing session key in trusted register response', Code.Internal);
		}
		if (timiteId !== undefined) {
			const timite = create(TimiteSchema, {
				id: timiteId,
				nick: this.conf.nick
			});
			this.storeTimite(timite);
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

	getTimite(): Timite | null {
		return this.timite;
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
