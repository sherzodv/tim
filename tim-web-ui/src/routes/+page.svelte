<script lang="ts">
import { browser } from '$app/environment';
import './+page.css';
import { session, THEME_OPTIONS } from '$lib/stores/session';
import type { Theme } from '$lib/models/session';
import { apiService } from '$lib/api/service';

const themeOptions = THEME_OPTIONS;

let commandInput = $state('');
let logContainer: HTMLElement | null = null;
let isOnline = $state(false);

const handleSubmit = (event: SubmitEvent) => {
	event.preventDefault();

	if (!isOnline) return;

	const trimmed = commandInput.trim();
	if (!trimmed) return;

	apiService.sendCommand(trimmed);

	commandInput = '';
};

const setTheme = (value: Theme) => {
	if (!isOnline) return;
	apiService.sendCommand(`THEME ${value}`);
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
	<title>Command Console</title>
	<meta name="description" content="Minimal command console interface" />
</svelte:head>

<div class="command-shell" class:offline={!isOnline} aria-busy={!isOnline}>
	<header class="shell-header">
		<h1 class="shell-title">Command Console</h1>
		<div class="theme-controls" role="group" aria-label="Theme selection" aria-disabled={!isOnline}>
			{#each themeOptions as option}
				<button
					type="button"
					class:selected={$session.theme === option.value}
					disabled={!isOnline}
					onclick={() => setTheme(option.value)}>
					{option.label}
				</button>
			{/each}
		</div>
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
					<div class="log-placeholder">Workspace is empty. Enter a command to begin.</div>
				{/if}
				{#each $session.entries as entry (entry.id)}
					<div
						class="log-entry"
						class:command={entry.role === 'command'}
						class:output={entry.role === 'output'}>
						<span class="log-prefix">{entry.role === 'command' ? 'Cmd' : 'App'}</span>
						<div
							class="log-text"
							class:text={entry.content.kind === 'text'}
							class:rich={entry.content.kind === 'html'}>
							{#if entry.content.kind === 'html'}
								{@html entry.content.html}
							{:else}
								{entry.content.text}
							{/if}
						</div>
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
		aria-label="Command input panel"
		aria-busy={!isOnline}>
		<label class="command-label" for="command-input">Command</label>
		<input
			id="command-input"
			name="command"
			type="text"
			autocomplete="off"
			spellcheck="false"
			bind:value={commandInput}
			placeholder="Enter command and press Enter"
			disabled={!isOnline}
		/>
	</form>

	<div class="help-line" role="note">{$session.help}</div>
</div>
