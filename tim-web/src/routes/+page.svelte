<script lang="ts">
	import { onMount } from 'svelte';
	import WorkSpace from '$lib/ui/work-space/WorkSpace.svelte';

	onMount(() => {
		console.info('[Tim] UI mounted');
	});
</script>

<svelte:head>
	<title>Tim</title>
</svelte:head>

<main class="blank-canvas" aria-label="Workspace">
	<WorkSpace />
</main>
