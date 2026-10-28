export interface AppendQueueState<T>{pending:readonly T[];saving:boolean;error:string}
/** Serial bounded appends preserve proposals and decisions arriving during generation. */
export class AppendQueue<T extends {id:string}>{
 private items:T[]=[];private flight:Promise<void>|null=null;private disposed=false;private error='';private completed=new Set<string>();
 constructor(private readonly append:(batch:T[])=>Promise<void>,private readonly changed:(state:AppendQueueState<T>)=>void){}
 enqueue(messages:readonly T[]):Promise<void>{if(this.disposed)return Promise.resolve();for(const message of messages)if(!this.completed.has(message.id)&&!this.items.some(item=>item.id===message.id))this.items.push(message);this.notify();return this.error?Promise.resolve():this.drain();}
 isCommitted(messages:readonly T[]):boolean{return messages.every(message=>this.completed.has(message.id));}
 retry():Promise<void>{this.error='';return this.drain();}
 private notify(){if(!this.disposed)this.changed({pending:[...this.items],saving:this.flight!==null,error:this.error});}
 private drain():Promise<void>{if(this.flight||this.disposed)return this.flight??Promise.resolve();this.error='';const work=async()=>{while(this.items.length&&!this.disposed){const batch:T[]=[];for(const item of this.items.slice(0,4)){if(batch.length&&new TextEncoder().encode(JSON.stringify([...batch,item])).length>8388608)break;batch.push(item);}try{await this.append(batch);}catch(error){if(!this.disposed)this.error=error instanceof Error?error.message:'讨论保存失败，请重试';break;}if(this.disposed)return;for(const item of batch)this.completed.add(item.id);const done=new Set(batch.map(item=>item.id));this.items=this.items.filter(item=>!done.has(item.id));this.notify();}};this.flight=Promise.resolve().then(work).finally(()=>{this.flight=null;this.notify();});this.notify();return this.flight;}
 dispose(){this.disposed=true;this.items=[];}
}
