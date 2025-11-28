<script lang="ts">
	import LeftPanel from './LeftPanel.svelte';
	import Worklog from './worklog/Worklog.svelte';
	import type { TimSpace } from '$lib/api/space';
	import type { TimStorage } from '$lib/api/storage';

	let { space, storage }: { space: TimSpace; storage: TimStorage } = $props();
</script>

<section class="workspace-shell tim-theme" aria-label="Workspace" data-space-active={space ? 'true' : 'false'}>
	<div class="workspace-body">
		<LeftPanel />
		<Worklog items={$storage} />
	</div>
</section>

<style>
	@import '$lib/ui/theme.css';

	.workspace-shell {
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		flex: 1 1 auto;
		min-height: 0;
		min-width: 0;
		overflow: hidden;
		background: var(--tim-surface-bg);
	}

	.workspace-body {
		display: flex;
		flex: 1 1 auto;
		min-height: 0;
		min-width: 0;
	}

	@media (prefers-color-scheme: dark) {
		.workspace-shell {
			background: var(--tim-surface-bg);
		}
	}
</style>
