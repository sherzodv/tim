<script lang="ts">
	import Worklog from './worklog/Worklog.svelte';
	import type { TimSpace } from '$lib/tim-space';
	import type { TimSpaceStorage } from '$lib/tim-space/storage';

	let { space, storage }: { space: TimSpace; storage: TimSpaceStorage } = $props();
</script>

<section class="workspace-shell tim-theme" aria-label="Workspace" data-space-active={space ? 'true' : 'false'}>
	<Worklog items={$storage} />
</section>

<style>
	@import '$lib/ui/theme.css';

	.workspace-shell {
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		height: 100vh;
		min-height: 100vh;
		min-width: 0;
		overflow: hidden;
		background: var(--tim-surface-bg);
	}

	@media (prefers-color-scheme: dark) {
		.workspace-shell {
			background: var(--tim-surface-bg);
		}
	}
</style>
