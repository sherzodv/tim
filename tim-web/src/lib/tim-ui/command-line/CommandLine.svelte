<section class="command-line tim-theme" aria-label="Command line">
	<div class="status-line" aria-label="Command status"></div>
	<div class="command-surface" aria-label="Command input area"></div>
</section>

<style>
	@import '$lib/tim-ui/theme.css';

	.command-line {
		position: sticky;
		bottom: 0;
		display: flex;
		flex-direction: column;
		width: 100%;
		background: var(--tim-surface-bg);
		border-top: 1px solid var(--tim-divider);
	}

	.status-line {
		min-height: 2.5rem;
		border-bottom: 1px solid var(--tim-divider);
	}

	.command-surface {
		min-height: 7rem;
	}
</style>
