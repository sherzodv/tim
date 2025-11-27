<script lang="ts">
	import WorkLog from '../work-log/WorkLog.svelte';
	import type { TimSpace } from '../../tim-space';
	import type { TimSpaceStorage } from '../../tim-space/storage';

	let { space, storage }: { space: TimSpace; storage: TimSpaceStorage } = $props();
</script>

<section class="work-space-shell tim-theme" aria-label="Work space" data-space-active={space ? 'true' : 'false'}>
	<aside class="chat-list" aria-label="Chat list placeholder"></aside>
	<section class="work-space">
		<WorkLog items={$storage} />
	</section>
</section>

<style>
	@import '../theme.css';

	.work-space-shell {
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

	.work-space {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
		background: var(--tim-surface-bg);
		overflow: hidden;
		height: 100%;
	}

	@media (prefers-color-scheme: dark) {
		.work-space-shell {
			background: var(--tim-surface-bg);
		}

		.chat-list {
			background: var(--tim-sidebar-bg);
			border-color: var(--tim-divider);
		}

		.work-space {
			background: var(--tim-surface-bg);
		}
	}
</style>
