<script lang="ts">
	import type { WorkLogItem } from './types';
	import Message from './item/Message.svelte';
	import SysMessage from './item/SysMessage.svelte';

	let { item }: { item: WorkLogItem } = $props();
</script>

{#if item.kind === 'msg'}
	<Message {item} />
{:else}
	<SysMessage {item} />
{/if}
