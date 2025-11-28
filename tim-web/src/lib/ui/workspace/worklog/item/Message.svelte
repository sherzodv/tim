<script lang="ts">
	import type { WorklogItemMessage } from '$lib/api/worklog';

	let { item }: { item: WorklogItemMessage } = $props();

	const avatarLabel = deriveAvatar(item.author);
	const avatarGradient = buildAvatarGradient(item.author);
	const authorColor = pickAuthorColor(item.author);

	function deriveAvatar(author: String): string {
		const text = `${author ?? ''}`.trim();
		if (!text) return '?';
		const firstWord = text.split(/\s+/)[0];
		const alpha = firstWord.replace(/[^A-Za-z0-9]/g, '');
		const candidate = alpha || firstWord;
		return candidate.charAt(0).toUpperCase();
	}

	function buildAvatarGradient(author: String): string {
		const base = hashHue(`${author ?? ''}`);
		const start = `hsl(${base}, 60%, 52%)`;
		const end = `hsl(${(base + 28) % 360}, 65%, 48%)`;
		return `linear-gradient(145deg, ${start}, ${end})`;
	}

	function pickAuthorColor(author: String): string {
		const base = hashHue(`${author ?? ''}`);
		return `hsl(${base}, 68%, 50%)`;
	}

	function hashHue(input: string): number {
		let hash = 0;
		for (let i = 0; i < input.length; i += 1) {
			hash = (hash * 31 + input.charCodeAt(i)) >>> 0;
		}
		return hash % 360;
	}
</script>

<article class="message-row" aria-label="Message row">
	<div class="avatar" aria-hidden="true" style={`background:${avatarGradient}`}>{avatarLabel}</div>
	<div class="worklog-item message" data-kind="message">
		<header>
			<span class="author" style={`color:${authorColor}`}>{item.author}</span>
			{#if item.time}
				<time class="timestamp" datetime={item.time}>{item.time}</time>
			{/if}
		</header>
		<p class="content">{item.content}</p>
	</div>
</article>

<style>
	.message-row {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 0.75rem;
		align-items: end;
		width: 100%;
		margin-bottom: 0.75rem;
	}

	.avatar {
		width: 48px;
		height: 48px;
		border-radius: 50%;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		font-weight: 800;
		color: var(--tim-avatar-text);
		box-shadow: var(--tim-avatar-shadow);
	}

	.worklog-item {
		display: inline-flex;
		flex-direction: column;
		gap: 0.35rem;
		padding: 0.9rem 1.05rem 0.95rem;
		max-width: min(78%, 640px);
		border-radius: 0.9rem 0.9rem 0.9rem 0.25rem;
		background: var(--tim-bubble-bg);
		border: 1px solid var(--tim-bubble-border);
		box-shadow: var(--tim-bubble-shadow);
	}

	header {
		display: flex;
		justify-content: space-between;
		font-size: 0.78rem;
		color: var(--tim-bubble-text);
		opacity: 0.9;
		text-transform: uppercase;
		letter-spacing: 0.06em;
	}

	.author {
		font-weight: 700;
		font-size: 0.95rem;
	}

	.timestamp {
		font-weight: 600;
		font-size: 0.78rem;
		color: var(--tim-bubble-text);
		opacity: 0.8;
	}

	.content {
		font-size: var(--tim-chat-font-size, 1.1rem);
		line-height: 1.4;
		margin: 0;
		color: var(--tim-bubble-text);
		white-space: pre-wrap;
		word-break: break-word;
	}
</style>
