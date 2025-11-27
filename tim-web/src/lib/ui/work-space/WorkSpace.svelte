<script lang="ts">
	import WorkLog from '../work-log/WorkLog.svelte';
	import type { TimSpace } from '../../tim-space';
	import type { TimSpaceStorage } from '../../tim-space/storage';

	let { space, storage }: { space: TimSpace; storage: TimSpaceStorage } = $props();
</script>

<section class="workspace-shell" aria-label="Workspace" data-space-active={space ? 'true' : 'false'}>
	<aside class="chat-list" aria-label="Chat list placeholder"></aside>
	<section class="work-space">
		<WorkLog items={$storage} />
	</section>
</section>

<style>
	.workspace-shell {
		display: grid;
		grid-template-columns: 320px 1fr;
		grid-template-rows: 1fr;
		height: 100vh;
		min-height: 100vh;
		min-width: 0;
		min-height: 0;
		background: var(--tg-surface-bg);
		color: var(--tg-surface-text);
		--tg-surface-bg: #e6e9ee;
		--tg-surface-text: #0f1419;
		--tg-bubble-bg: #ffffff;
		--tg-bubble-border: #d1d6dd;
		--tg-bubble-shadow: 0 6px 18px rgba(0, 0, 0, 0.07);
		--tg-bubble-text: #0a0d12;
		--tg-sys-bg: #eef1f5;
		--tg-sys-border: #d8dde3;
		--tg-sys-text: #222a33;
		--tg-sidebar-bg: #f4f6f9;
		--tg-divider: #d6dbe1;
		--tg-chat-font-size: 1.5rem;
		--tg-avatar-bg: #d5dae1;
		--tg-avatar-text: #0f1419;
	}

	.chat-list {
		border-right: 1px solid var(--tg-divider);
		background: var(--tg-sidebar-bg);
		height: 100%;
		min-height: 0;
	}

	.work-space {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
		background: var(--tg-surface-bg);
		color: var(--tg-surface-text);
		overflow: hidden;
		height: 100%;
	}

	@media (prefers-color-scheme: dark) {
		.workspace-shell {
			--tg-surface-bg: #11161c;
			--tg-surface-text: #f9fbff;
			--tg-bubble-bg: #1e252e;
			--tg-bubble-border: #2c343f;
			--tg-bubble-shadow: 0 12px 28px rgba(0, 0, 0, 0.48);
			--tg-bubble-text: #ffffff;
			--tg-sys-bg: #1a212b;
			--tg-sys-border: #2b323c;
			--tg-sys-text: #ffffff;
			--tg-sidebar-bg: #161c23;
			--tg-divider: #252c36;
			--tg-chat-font-size: 1.3rem;
			--tg-avatar-bg: #2b323c;
			--tg-avatar-text: #ffffff;
		}

		.chat-list,
		.work-space {
			background: var(--tg-surface-bg);
		}
	}
</style>
