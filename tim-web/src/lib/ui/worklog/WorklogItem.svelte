<script lang="ts">
	import type { WorklogItem } from './types';
	import Message from './item/Message.svelte';
	import SysMessage from './item/SysMessage.svelte';

	let { item }: { item: WorklogItem } = $props();
</script>

{#if item.kind === 'msg'}
	<Message {item} />
{:else}
	<SysMessage {item} />
{/if}
