<script lang="ts">
	import WorkLog from '../work-log/WorkLog.svelte';
	import type { WorkLogItem } from '../work-log/types';

	let sampleItems: WorkLogItem[] = $state([]);

	let idx = 0;
	setInterval(() => {
		idx += 1;
		sampleItems.push({
			id: BigInt(idx),
			kind: idx % 2 === 0 ? 'msg' : 'sysmsg',
			author: idx % 2 === 0 ? 'alice' : 'jarvis',
			content: `Placeholder message #${idx}`
		});
		sampleItems = sampleItems;
	}, 500);
</script>

<section class="work-space" aria-label="Workspace">
	<WorkLog items={sampleItems} />
</section>

<style>
	.work-space {
		display: flex;
		flex-direction: column;
		flex: 1;
		height: 100vh;
		min-height: 100vh;
		background:
			radial-gradient(circle at top left, rgba(95, 200, 255, 0.12), transparent 40%),
			linear-gradient(180deg, #05060a, #0c0f17);
		color: #f3f7ff;
		overflow-y: none;
	}
</style>
