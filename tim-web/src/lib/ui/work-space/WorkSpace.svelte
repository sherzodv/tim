<script lang="ts">
	import WorkLog from '../work-log/WorkLog.svelte';
	import type { TimSpace } from '../../tim-space';
	import type { TimSpaceStorage } from '../../tim-space/storage';

	let { space, storage }: { space: TimSpace; storage: TimSpaceStorage } = $props();
</script>

<section class="work-space" aria-label="Workspace" data-space-active={space ? 'true' : 'false'}>
	<WorkLog items={$storage} />
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
