<aside class="left-panel tim-theme" aria-label="Navigation"></aside>

<style>
	@import '$lib/ui/theme.css';

	.left-panel {
		box-sizing: border-box;
		flex: 0 0 480px;
		min-height: 0;
		border-right: 1px solid var(--tim-divider);
		background: var(--tim-sidebar-bg);
	}

	@media (max-width: 960px) {
		.left-panel {
			flex-basis: 220px;
		}
	}
</style>
