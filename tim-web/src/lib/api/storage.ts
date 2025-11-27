import { writable, type Readable } from 'svelte/store';
import type { WorklogItem } from './worklog';

export type TimStorage = Readable<WorklogItem[]> & {
	append(item: WorklogItem): void;
	set(items: WorklogItem[]): void;
	reset(): void;
};

export const createTimStorage = (): TimStorage => {
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
