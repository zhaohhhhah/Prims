export function spreadStart(page:number,step:number,total:number):number{return Math.max(0,Math.min(Math.floor((Math.max(1,total)-1)/step)*step,Math.floor(page/step)*step));}
export function spreadPages(start:number,step:number,total:number):number[]{return Array.from({length:step},(_,i)=>start+i+1).filter(n=>n<=total);}
export function bookTurnPages(previous:number,target:number,step:number){const forward=target>previous;return {forward,front:previous+(step===1?1:forward?2:1),back:target+(step===1?1:forward?1:2),stationary:previous+(forward?1:2)};}
