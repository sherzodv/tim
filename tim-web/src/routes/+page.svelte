<script lang="ts">
	import { browser } from '$app/environment';
	import WorkSpace from '$lib/ui/work-space/WorkSpace.svelte';
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

	$effect(() => {
		if (!browser) return;
		console.info('[Tim] UI mounted');
		timSpace.start();
		return () => {
			timSpace.stop();
		};
	});
</script>

<svelte:head>
	<title>Tim</title>
</svelte:head>

<main class="blank-canvas" aria-label="Workspace">
	<WorkSpace space={timSpace} storage={spaceStorage} />
</main>
