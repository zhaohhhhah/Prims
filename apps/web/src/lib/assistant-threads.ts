export interface SortableAssistantThread {id:string;lastMessageAt?:string|null;createdAt?:string;updatedAt:string}
function time(value:string|undefined|null):number{const result=value?Date.parse(value):0;return Number.isFinite(result)?result:0;}
/** Selecting a thread never contributes to ordering; only persisted message time does. */
export function sortAssistantThreads<T extends SortableAssistantThread>(threads:readonly T[]):T[]{return [...threads].sort((a,b)=>time(b.lastMessageAt??b.createdAt??b.updatedAt)-time(a.lastMessageAt??a.createdAt??a.updatedAt)||a.id.localeCompare(b.id));}
