<script lang="ts">
	import { browser } from '$app/environment';
	import Workspace from '$lib/ui/workspace/Workspace.svelte';
	import CommandLine from '$lib/ui/command-line/CommandLine.svelte';
	import { createTimClient } from '$lib/api/client';
	import { createTimConnect } from '$lib/api/connect';
	import { createTimSpace } from '$lib/api/space';
	import { createTimStorage } from '$lib/api/storage';

	const timClient = createTimClient({
		nick: 'bob',
		platform: 'browser'
	});
	const timConnect = createTimConnect(timClient);
	const timStorage = createTimStorage();
	const timSpace = createTimSpace(timClient, timConnect, timStorage);

const lorem = [
	'Lorem ipsum dolor sit amet, consectetur adipiscing elit.',
	'Vivamus fermentum nunc nec urna porttitor, vel tempus mauris cursus.',
	'Integer non arcu vitae ipsum interdum pulvinar.',
	'Curabitur sit amet nisl quis lorem dignissim posuere.',
	'Donec auctor, sem nec placerat aliquet, justo odio posuere urna, eget facilisis nulla nisl in metus.'
];

function startLoremFeed(enabled: boolean, intervalMs = 1400) {
	if (!enabled) return () => {};
	let canceled = false;
	const pushLorem = () => {
		if (canceled) return;
		const idx = Math.floor(Math.random() * lorem.length);
		const id = BigInt(Date.now());
		timStorage.append({
			kind: 'msg',
			id,
			author: 'tester',
			content: lorem[idx],
			time: new Date().toISOString()
		});
		setTimeout(pushLorem, intervalMs);
	};
	pushLorem();
	return () => {
		canceled = true;
	};
}

$effect(() => {
	if (!browser) return;
	console.info('[Tim] UI mounted');
	timSpace.start();
	const stopLorem = startLoremFeed(false, 1400);
	return () => {
		stopLorem();
		timSpace.stop();
	};
});
</script>

<svelte:head>
	<title>Tim</title>
</svelte:head>

<main class="page-shell tim-theme" aria-label="Workspace">
	<div class="workspace-region">
		<Workspace space={timSpace} storage={timStorage} />
	</div>
	<CommandLine space={timSpace} />
</main>

<style>
	@import '$lib/ui/theme.css';

	.page-shell {
		height: 100vh;
		min-height: 0;
		display: flex;
		flex-direction: column;
		background: var(--tim-surface-bg);
	}

	.workspace-region {
		flex: 1 1 auto;
		min-height: 0;
		display: flex;
	}
</style>
