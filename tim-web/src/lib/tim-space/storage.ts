import { writable, type Readable } from 'svelte/store';
import type { WorkLogItem } from '../ui/work-log/types';

export type TimSpaceStorage = Readable<WorkLogItem[]> & {
	append(item: WorkLogItem): void;
	reset(): void;
};

export const createTimSpaceStorage = (): TimSpaceStorage => {
	const { subscribe, update, set } = writable<WorkLogItem[]>([]);

	return {
		subscribe,
		append(item) {
			update((items) => [...items, item]);
		},
		reset() {
			set([]);
		}
	};
};
