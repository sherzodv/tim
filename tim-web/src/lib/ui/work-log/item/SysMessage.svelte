<script lang="ts">
	import type { WorkLogItemSysMessage } from '../types';

	let { item }: { item: WorkLogItemSysMessage } = $props();
</script>

<article class="sys-row" aria-label="System message">
	<div class="sys-pill">
		<span class="content">{item.content}</span>
	</div>
</article>

<style>
	.sys-row {
		display: flex;
		justify-content: center;
		margin: 0.35rem 0;
	}

	.sys-pill {
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.4rem 0.9rem;
		border-radius: 999px;
		background: rgba(34, 32, 32, 0.239);
		border: 1px solid rgba(0, 0, 0, 0.12);
		color: rgba(179, 184, 190, 0.95);
		font-size: 1.02rem;
		font-weight: 800;
		letter-spacing: 0.01em;
		text-transform: capitalize;
		box-shadow: none;
	}

	.content {
		margin: 0;
		white-space: pre-wrap;
		word-break: break-word;
	}
</style>
