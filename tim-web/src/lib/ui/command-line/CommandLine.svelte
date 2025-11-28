<script lang="ts">
	import type { TimSpace } from '$lib/api/space';
	import { onMount } from 'svelte';

	let { space }: { space: TimSpace } = $props();

	let inputValue = $state('');
	let inputEl: HTMLTextAreaElement;

	$effect(() => {
		inputEl?.focus();
	});

	function autoResize() {
		if (!inputEl) return;
		inputEl.style.height = '0';
		inputEl.style.height = inputEl.scrollHeight + 'px';
	}

	$effect(() => {
		if (inputValue !== undefined) {
			autoResize();
		}
	});

	onMount(() => {
		autoResize();

		const handleGlobalKeyDown = (event: KeyboardEvent) => {
			// Don't interfere with special keys or when modifiers are pressed (except Shift for typing)
			if (
				event.ctrlKey ||
				event.metaKey ||
				event.altKey ||
				event.key === 'Tab' ||
				event.key === 'Escape' ||
				event.key.startsWith('F')
			) {
				return;
			}

			// If typing a printable character and input is not focused, focus it
			if (event.key.length === 1 && document.activeElement !== inputEl) {
				inputEl?.focus();
			}
		};

		window.addEventListener('keydown', handleGlobalKeyDown);

		return () => {
			window.removeEventListener('keydown', handleGlobalKeyDown);
		};
	});

	function handleKeyDown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey) {
			event.preventDefault();
			sendMessage();
		}
	}

	async function sendMessage() {
		const content = inputValue.trim();
		if (!content) return;

		try {
			await space.send(content);
			inputValue = '';
			inputEl?.focus();
		} catch (error) {
			console.error('Failed to send message:', error);
		}
	}
</script>

<section class="command-line tim-theme" aria-label="Command line">
	<div class="status-line" aria-label="Command status"></div>
	<div class="command-surface" aria-label="Command input area">
		<textarea
			bind:this={inputEl}
			bind:value={inputValue}
			onkeydown={handleKeyDown}
			oninput={autoResize}
			placeholder="Type a message... (Enter to send, Shift+Enter for new line)"
			class="command-input"
			rows="1"
		></textarea>
		<button onclick={sendMessage} class="send-button" disabled={!inputValue.trim()}>Send</button>
	</div>
</section>

<style>
	@import '$lib/ui/theme.css';

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
		display: flex;
		gap: 0.75rem;
		padding: 1rem;
		align-items: flex-start;
	}

	.command-input {
		flex: 1 1 auto;
		padding: 0.75rem;
		border: 1px solid var(--tim-divider);
		border-radius: 4px;
		background: var(--tim-surface-bg);
		color: var(--tim-surface-text);
		font-family: var(--tim-font-family);
		font-size: var(--tim-font-size);
		line-height: var(--tim-line-height);
		resize: none;
		outline: none;
		overflow: hidden;
		max-height: 200px;
		box-sizing: border-box;
		min-height: 0;
	}

	.command-input:focus {
		border-color: var(--tim-primary, #4a90e2);
		box-shadow: 0 0 0 2px rgba(74, 144, 226, 0.1);
	}

	.command-input::placeholder {
		color: var(--tim-surface-text);
		opacity: 0.5;
	}

	.send-button {
		display: none;
	}
</style>
