<script lang="ts">
	import type { WorklogItem } from '$lib/api/worklog';
	import WorklogItemCmp from './item/index.svelte';
	import WorklogHeader from './WorklogHeader.svelte';

	let { items }: { items: WorklogItem[] } = $props();

	let listEl: HTMLElement;
	let atBottom = true;

	$effect(() => {
		// re-run when list changes
		const _count = items.length;
		if (atBottom && listEl) {
			queueMicrotask(() => {
				listEl.scrollTop = listEl.scrollHeight;
			});
		}
	});

	function handleScroll(event: Event) {
		const target = event.currentTarget as HTMLElement | null;
		if (!target) return;
		const distance = target.scrollHeight - (target.scrollTop + target.clientHeight);
		atBottom = distance < 48;
	}
</script>

<section class="worklog-container">
	<WorklogHeader />
	<div class="worklog" bind:this={listEl} aria-live="polite" onscroll={handleScroll}>
		{#each items as item, i (`${item.kind}-${item.id}-${i}`)}
			<WorklogItemCmp entry={item} />
		{/each}
	</div>
</section>

<style>
	@import '$lib/tim-ui/theme.css';

	.worklog-container {
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		flex: 1 1 auto;
		width: 100%;
		height: 100%;
		min-height: 0;
		max-height: 100%;
		background: var(--tim-surface-bg);
	}

	.worklog {
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		flex: 1 1 auto;
		min-height: 0;
		overflow-y: scroll;
		scrollbar-gutter: stable both-edges;
		padding: 1.75rem;
		background: var(--tim-surface-bg);
		color: var(--tim-surface-text);
		font-family: var(--tim-font-family);
		font-size: var(--tim-font-size);
		line-height: var(--tim-line-height);
		scrollbar-width: auto;
		scrollbar-color: var(--tim-scrollbar-thumb) var(--tim-scrollbar-track);
	}

	.worklog::-webkit-scrollbar {
		width: 16px;
	}

	.worklog::-webkit-scrollbar-track {
		background: var(--tim-scrollbar-track);
	}

	.worklog::-webkit-scrollbar-thumb {
		background: var(--tim-scrollbar-thumb);
		border-radius: 10px;
		border: 2px solid var(--tim-surface-bg);
	}

	.worklog::-webkit-scrollbar-thumb:hover {
		background: var(--tim-scrollbar-thumb-hover);
	}
</style>
