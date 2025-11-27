<script lang="ts">
	import type { WorkLogItemSysMessage } from '../types';

	let { item }: { item: WorkLogItemSysMessage } = $props();
</script>

<article class="work-log-item message" data-kind="sysmsg" aria-label="System message">
	<header>
		<span class="author">{item.author}</span>
		<time datetime="">
			{item.kind}
		</time>
	</header>
	<p class="content">{item.content}</p>
</article>

<style>
	.work-log-item {
		display: inline-flex;
		flex-direction: column;
		gap: 0.3rem;
		align-self: center;
		padding: 0.7rem 1rem;
		max-width: min(70%, 460px);
		border-radius: 1rem;
		background: var(--tg-sys-bg);
		border: 1px solid var(--tg-sys-border);
		box-shadow: 0 6px 14px rgba(0, 0, 0, 0.08);
		color: var(--tg-sys-text);
	}

	header {
		display: flex;
		justify-content: space-between;
		font-size: 0.72rem;
		color: var(--tg-sys-text);
		opacity: 0.95;
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}

	.author {
		font-weight: 700;
	}

	.content {
		font-size: var(--tg-chat-font-size, 1.1rem);
		line-height: 1.35;
		margin: 0;
		color: var(--tg-sys-text);
		white-space: pre-wrap;
		word-break: break-word;
	}
</style>
