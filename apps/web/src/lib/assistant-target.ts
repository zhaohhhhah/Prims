/** A stale anchor may support a new discussion, but can never authorize another edit. */
export function assistantRequestTarget<T extends {text:string;conflict:boolean}>(candidate:T|null|undefined,documentReadOnly:boolean){const target=candidate&&!candidate.conflict?candidate:null;return {target,selection:target?.text??'',readOnly:documentReadOnly||target===null};}
