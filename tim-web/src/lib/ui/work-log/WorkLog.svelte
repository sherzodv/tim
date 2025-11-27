<script lang="ts">
	import { createVirtualizer } from '@tanstack/svelte-virtual';
	import type { WorkLogItem } from './types';
	import WorkLogItemC from './WorkLogItem.svelte';

	let { items }: { items: WorkLogItem[] } = $props();

	console.log("hello")

	let virtualList: HTMLElement;
	let virtualElems: HTMLDivElement[] = $state([]);

	const v = createVirtualizer({
		count: 0,
		estimateSize: () => 10,
		getScrollElement: () => virtualList ?? null,
		overscan: 5
	});

	$effect(() => {
		if ($v.options.count !== items.length) {
			$v.setOptions({ count: items.length });
			virtualElems.forEach((el) => $v.measureElement(el));
		}
	});
</script>

<section class="work-log" bind:this={virtualList} aria-live="polite">
	<div
		class="work-log-body"
		style={`height:${$v.getTotalSize()}px; position:relative; width: 100%`}
	>
		{#each $v.getVirtualItems() as vi, idx (vi.index)}
			<div
				bind:this={virtualElems[idx]}
				data-index={vi.index}
				style="position: absolute; top: 0; left: 0; width: 100%; transform: translateY({vi.start}px);"
			>
				<WorkLogItemC item={items[vi.index]} />
			</div>
		{/each}
	</div>
</section>

<style>
	.work-log {
		display: block;
		flex: 1;
		width: 100%;
		height: 100%;
		min-height: 100%;
		overflow-y: auto;
		padding: 1.5rem;
	}

	.work-log-body {
		display: block;
		position: relative;
		width: 100%;
	}
</style>
