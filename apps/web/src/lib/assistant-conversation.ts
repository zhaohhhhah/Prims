export interface AssistantScrollMetrics {scrollTop:number;scrollHeight:number;clientHeight:number}
export function assistantBottomDistance(value:AssistantScrollMetrics):number{return Math.max(0,value.scrollHeight-value.clientHeight-value.scrollTop);}
/** Follow the latest turn until the reader moves upward; no message/token can override that choice. */
export class AssistantScrollPolicy {
 private following=true;
 private lastTop=0;
 get shouldFollow():boolean{return this.following;}
 reset():void{this.following=true;this.lastTop=0;}
 pause():void{this.following=false;}
 scrolledToEnd(value:AssistantScrollMetrics):void{this.lastTop=value.scrollTop;}
 onScroll(value:AssistantScrollMetrics):void{
  const delta=value.scrollTop-this.lastTop,distance=assistantBottomDistance(value);
  if(delta < -1&&distance>1)this.following=false;
  else if(delta>1&&distance<=64)this.following=true;
  else if(distance>64)this.following=false;
  this.lastTop=value.scrollTop;
 }
}
export function showAssistantTurnProgress(busy:boolean,diagnosticPending:boolean,phase:string):boolean{return busy||diagnosticPending||['stopped','partial','error'].includes(phase);}
