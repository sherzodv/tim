import { writable, type Readable } from 'svelte/store';
import type { WorklogItem } from '../ui/worklog/types';

export type TimSpaceStorage = Readable<WorklogItem[]> & {
	append(item: WorklogItem): void;
	set(items: WorklogItem[]): void;
	reset(): void;
};

export const createTimSpaceStorage = (): TimSpaceStorage => {
	const { subscribe, update, set } = writable<WorklogItem[]>([]);

	return {
		subscribe,
		append(item) {
			update((items) => [...items, item]);
		},
		set(items) {
			set([...items]);
		},
		reset() {
			set([]);
		}
	};
};
