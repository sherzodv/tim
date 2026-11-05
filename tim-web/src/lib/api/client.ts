import { createClient, type Client } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';
import { ConnectError, Code } from '@connectrpc/connect';
import { create } from '@bufbuild/protobuf';
import {
	ErrorCode,
	TimGrpcApi,
	TrustedConnectReqSchema,
	ClientInfoSchema,
	SendMessageReqSchema,
	SendActivityReqSchema,
	ActivitySchema,
	SubscribeToSpaceReqSchema,
	GetTimelineReqSchema,
	TimiteSchema,
	type Timite,
	type TrustedConnectReq,
	type SendMessageReq,
	type SubscribeToSpaceReq,
	type GetTimelineRes
} from '../../gen/tim/api/g1/api_pb';

const SESSION_HEADER = 'tim-session-key' as const;
const BASE_URL = import.meta.env.VITE_TIM_CODE_URL ?? 'http://127.0.0.1:8787';

export type TimClientConf = {
	/** Unique handle the timite connects as and is addressed by. */
	username: string;
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

	/** Opens a session as the configured username; the server creates the timite on first use. */
	private async acquireSession(): Promise<string> {
		const response = await this.client.trustedConnect(buildConnectRequest(this.conf));
		if (response.error !== ErrorCode.UNSPECIFIED) {
			throw new ConnectError(
				`connection rejected: ${ErrorCode[response.error]}`,
				Code.InvalidArgument
			);
		}
		const session = response.session;
		if (!session) {
			throw new ConnectError('missing session in trusted connect response', Code.Internal);
		}
		this.timite = create(TimiteSchema, {
			id: session.timiteId,
			username: this.conf.username
		});
		return session.key;
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

	/** Shares transient state such as "typing"; delivered live, never stored. */
	async sendActivity(kind: string, payload = ''): Promise<void> {
		const sessionKey = await this.ensureSession();
		const request = create(SendActivityReqSchema, {
			activity: create(ActivitySchema, { kind, payload })
		});
		await this.client.sendActivity(request, {
			headers: buildSessionHeaders(sessionKey)
		});
	}

	async subscribeToSpace(
		receiveOwnMessages: boolean,
		afterId: bigint | undefined,
		signal: AbortSignal
	) {
		const sessionKey = await this.ensureSession();
		const request = buildSubscribeRequest(receiveOwnMessages, afterId);
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

function buildConnectRequest(conf: TimClientConf): TrustedConnectReq {
	const clientInfo = create(ClientInfoSchema, {
		platform: conf.platform
	});
	return create(TrustedConnectReqSchema, {
		username: conf.username,
		clientInfo
	});
}

function buildSendMessageRequest(content: string): SendMessageReq {
	return create(SendMessageReqSchema, {
		content
	});
}

function buildSubscribeRequest(
	receiveOwnMessages: boolean,
	afterId: bigint | undefined
): SubscribeToSpaceReq {
	return create(SubscribeToSpaceReqSchema, {
		receiveOwnMessages,
		afterId
	});
}

function buildSessionHeaders(sessionId: bigint | string): HeadersInit {
	return [[SESSION_HEADER, sessionId.toString()]];
}
