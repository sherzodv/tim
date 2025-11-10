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
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		padding: 0.85rem 1rem;
		border-radius: 0.75rem;
		background: rgba(255, 255, 255, 0.02);
		border: 1px solid rgba(255, 255, 255, 0.05);
		box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
	}

	header {
		display: flex;
		justify-content: space-between;
		font-size: 0.75rem;
		color: rgba(255, 255, 255, 0.65);
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}

	.author {
		font-weight: 600;
	}

	.content {
		font-size: 0.95rem;
		line-height: 1.4;
		margin: 0;
		color: rgba(255, 255, 255, 0.92);
		white-space: pre-wrap;
		word-break: break-word;
	}
</style>
