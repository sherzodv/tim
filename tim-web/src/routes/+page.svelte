<script lang="ts">
	import { onMount } from 'svelte';
	import { createTimClient, createWebTimClientConf } from '$lib/tim-client';
	import { createTimConnect } from '$lib/tim-connect';
	import { createTimSpace } from '$lib/tim-space';

	const timClient = createTimClient(createWebTimClientConf());
	const timConnect = createTimConnect(timClient);
	const timSpace = createTimSpace(timClient);

	onMount(() => {
		timConnect.start(timSpace).catch((error) => {
			console.error('Failed to start TimConnect', error);
		});
		return () => {
			timConnect.stop();
		};
	});
</script>

<svelte:head>
		<title>Tim</title>
	</svelte:head>

	<main class="blank-canvas" aria-label="Empty workspace"></main>
