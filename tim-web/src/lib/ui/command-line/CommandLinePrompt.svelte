<script lang="ts">
	let { timite }: { timite?: string | null } = $props();
</script>

<div class="command-line-prompt">
	<span class="prompt-symbol">Tim Space CLI›</span>
	{#if timite}
		<span class="timite-handle">{timite}</span>
	{/if}
</div>

<style>
	.command-line-prompt {
		display: flex;
		align-items: flex-start;
		justify-content: flex-end;
		user-select: none;
		flex: 0 0 15%;
		gap: 0.35rem;
	}

	.prompt-symbol {
		font-family: var(--tim-font-family);
		font-size: var(--tim-input-font-size);
		line-height: var(--tim-line-height);
		color: var(--tim-surface-text);
		opacity: 0.6;
		padding: 0.75rem 0.75rem 0.75rem 0;
		display: inline-block;
		transform: translateY(0.08em);
	}

	.timite-handle {
		font-family: var(--tim-font-family);
		font-size: var(--tim-input-font-size);
		line-height: var(--tim-line-height);
		color: var(--tim-surface-text);
		opacity: 0.8;
		padding: 0.75rem 0.75rem 0.75rem 0;
		display: inline-block;
		transform: translateY(0.08em);
	}
</style>
