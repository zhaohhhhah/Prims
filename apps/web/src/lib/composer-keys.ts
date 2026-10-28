export interface ComposerKey {key:string;shiftKey?:boolean;nativeEvent:{isComposing?:boolean;keyCode?:number}}
export function isComposerComposing(event:ComposerKey):boolean{return event.nativeEvent.isComposing===true||event.nativeEvent.keyCode===229;}
export function submitsComposer(event:ComposerKey):boolean{return event.key==='Enter'&&!event.shiftKey&&!isComposerComposing(event);}
