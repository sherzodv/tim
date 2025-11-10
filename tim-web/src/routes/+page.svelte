<script lang="ts">
import { browser } from '$app/environment';
import './+page.css';
import { session } from '$lib/stores/session';
import type { Theme } from '$lib/models/session';
import { apiService } from '$lib/api/service';

let commandInput = $state('');
let logContainer: HTMLElement | null = null;
let isOnline = $state(false);
const clientId = browser ? apiService.getClientId() : 'system';

const handleSubmit = (event: SubmitEvent) => {
	event.preventDefault();

	if (!isOnline) return;

	const trimmed = commandInput.trim();
	if (!trimmed) return;

	apiService.sendMessage(trimmed);

	commandInput = '';
};

const applyTheme = (value: Theme) => {
	if (!browser) return;
	document.documentElement.dataset.theme = value;
};

$effect(() => {
	const currentTheme = $session.theme;
	applyTheme(currentTheme);
});

$effect(() => {
	// Track workspace updates to trigger scrolling when new entries are added
	$session.entries.length;

	if (!logContainer) return;

	logContainer.scroll({
		top: logContainer.scrollHeight,
		behavior: 'smooth'
	});
});

$effect(() => {
	isOnline = $session.connection === 'open';
});

// All status/help/theme updates flow from server responses
</script>

<svelte:head>
	<title>Message Console</title>
	<meta name="description" content="Minimal realtime messaging console" />
</svelte:head>

<div class="command-shell" class:offline={!isOnline} aria-busy={!isOnline}>
	<header class="shell-header">
		<h1 class="shell-title">Message Console</h1>
	</header>

		{#if !isOnline}
			<div class="connection-overlay" role="alert" aria-live="assertive">
				<div class="connection-overlay__panel">
					<p class="connection-overlay__status">{$session.status}</p>
					<p class="connection-overlay__hint">{$session.help}</p>
				</div>
			</div>
		{/if}

		<section class="workspace" bind:this={logContainer} aria-live="polite">
			<div class="workspace-content">
				{#if $session.entries.length === 0}
					<div class="log-placeholder">Workspace is empty. Send a message to begin.</div>
				{/if}
					{#each $session.entries as entry (entry.id)}
						<div
							class="log-entry"
							class:self={entry.senderId === clientId}
							class:remote={entry.senderId !== clientId}>
							<span class="log-prefix">
								{entry.senderId === clientId ? 'You' : entry.senderId}
							</span>
							<div class="log-text">{entry.content}</div>
						</div>
					{/each}
				</div>
		</section>

	<div class="status-line" role="status" aria-live="polite">
		<span class="status-label">Status</span>
		<span class="status-text">{$session.status}</span>
	</div>

	<form
		class="command-bar"
		onsubmit={handleSubmit}
		aria-label="Message input panel"
		aria-busy={!isOnline}>
		<label class="command-label" for="command-input">Message</label>
		<input
			id="command-input"
			name="command"
			type="text"
			autocomplete="off"
			spellcheck="false"
			bind:value={commandInput}
			placeholder="Enter message and press Enter"
			disabled={!isOnline}
		/>
	</form>

	<div class="help-line" role="note">{$session.help}</div>
</div>
