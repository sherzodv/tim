<script lang="ts">
	import type { WorklogItem } from '../../../../api/worklog';
	import Message from './Message.svelte';
	import SysMessage from './SysMessage.svelte';

	let { entry }: { entry: WorklogItem } = $props();
</script>

{#if entry.kind === 'msg'}
	<Message item={entry} />
{:else}
	<SysMessage item={entry} />
{/if}
