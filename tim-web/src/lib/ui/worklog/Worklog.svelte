<script lang="ts">
	import type { WorklogItem } from './types';
	import WorklogItemC from './WorklogItem.svelte';

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

<section class="worklog" bind:this={listEl} aria-live="polite" onscroll={handleScroll}>
	<div class="worklog-body">
		{#each items as item, i (item.kind + '-' + item.id + '-' + i)}
			<div class="log-row">
				<WorklogItemC {item} />
			</div>
		{/each}
	</div>
</section>

<style>
	@import '../theme.css';

	.worklog {
		display: block;
		flex: 1 1 auto;
		width: 100%;
		height: 100%;
		min-height: 0;
		max-height: 100%;
		overflow-y: auto;
		padding: 1.75rem;
		background: var(--tim-surface-bg);
		color: var(--tim-surface-text);
		font-family: var(--tim-font-family);
		font-size: var(--tim-font-size);
		line-height: var(--tim-line-height);
	}

	.worklog-body {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		width: 100%;
	}

	.log-row {
		width: 100%;
	}
</style>
