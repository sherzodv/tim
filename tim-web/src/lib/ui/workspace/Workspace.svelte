<script lang="ts">
	import Worklog from '../worklog/Worklog.svelte';
	import type { TimSpace } from '../../tim-space';
	import type { TimSpaceStorage } from '../../tim-space/storage';

	let { space, storage }: { space: TimSpace; storage: TimSpaceStorage } = $props();
</script>

<section class="workspace-shell tim-theme" aria-label="Workspace" data-space-active={space ? 'true' : 'false'}>
	<aside class="chat-list" aria-label="Chat list placeholder"></aside>
	<section class="workspace">
		<Worklog items={$storage} />
	</section>
</section>

<style>
	@import '../theme.css';

	.workspace-shell {
		display: grid;
		grid-template-columns: 320px 1fr;
		grid-template-rows: 1fr;
		height: 100vh;
		min-height: 100vh;
		min-width: 0;
		background: var(--tim-surface-bg);
	}

	.chat-list {
		border-right: 1px solid var(--tim-divider);
		background: var(--tim-sidebar-bg);
		height: 100%;
		min-height: 0;
	}

	.workspace {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
		background: var(--tim-surface-bg);
		overflow: hidden;
		height: 100%;
	}

	@media (prefers-color-scheme: dark) {
		.workspace-shell {
			background: var(--tim-surface-bg);
		}

		.chat-list {
			background: var(--tim-sidebar-bg);
			border-color: var(--tim-divider);
		}

		.workspace {
			background: var(--tim-surface-bg);
		}
	}
</style>
