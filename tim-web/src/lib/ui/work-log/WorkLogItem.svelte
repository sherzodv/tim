<script lang="ts">
	import type { WorkLogItem } from './types';
	import Message from './item/Message.svelte';
	import SysMessage from './item/SysMessage.svelte';

	export let item: WorkLogItem;

	const map = {
		msg: Message,
		sysmsg: SysMessage
	} as const;
</script>

<svelte:component this={map[item.kind]} item={item as any} />
