<script lang="ts">
	import { browser } from '$app/environment';
	import Workspace from '$lib/ui/workspace/Workspace.svelte';
	import { createTimClient } from '$lib/tim-client';
	import { createTimConnect } from '$lib/tim-connect';
	import { createTimSpace } from '$lib/tim-space';
	import { createTimSpaceStorage } from '$lib/tim-space/storage';

	const timClient = createTimClient({
		nick: 'bob',
		platform: 'browser'
	});
const timConnect = createTimConnect(timClient);
const spaceStorage = createTimSpaceStorage();
const timSpace = createTimSpace(timClient, timConnect, spaceStorage);

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
		spaceStorage.append({
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
	const stopLorem = startLoremFeed(true, 1400);
	return () => {
		stopLorem();
		timSpace.stop();
	};
});
</script>

<svelte:head>
	<title>Tim</title>
</svelte:head>

<main aria-label="Workspace">
	<Workspace space={timSpace} storage={spaceStorage} />
</main>
