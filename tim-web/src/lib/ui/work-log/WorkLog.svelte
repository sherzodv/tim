<script lang="ts">
	import type { WorkLogItem } from './types';
	import WorkLogItemC from './WorkLogItem.svelte';

	let { items }: { items: WorkLogItem[] } = $props();

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

<section class="work-log" bind:this={listEl} aria-live="polite" onscroll={handleScroll}>
	<div class="work-log-body">
		{#each items as item, i (item.kind + '-' + item.id + '-' + i)}
			<div class="log-row">
				<WorkLogItemC {item} />
			</div>
		{/each}
	</div>
</section>

<style>
	.work-log {
		display: block;
		flex: 1 1 auto;
		width: 100%;
		height: 100%;
		min-height: 0;
		max-height: 100%;
		overflow-y: auto;
		padding: 1.75rem;
		background: var(--tg-surface-bg);
		color: var(--tg-surface-text);
	}

	.work-log-body {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		width: 100%;
	}

	.log-row {
		width: 100%;
	}
</style>
