import ReactMarkdown, {type Components} from 'react-markdown';
import remarkGfm from 'remark-gfm';
const components:Components={
 h1:({children})=><h1 className="mb-2 mt-3 text-base font-semibold first:mt-0">{children}</h1>,
 h2:({children})=><h2 className="mb-2 mt-3 text-sm font-semibold first:mt-0">{children}</h2>,
 h3:({children})=><h3 className="mb-1.5 mt-3 text-sm font-medium first:mt-0">{children}</h3>,
 h4:({children})=><h4 className="mb-1 mt-2 font-semibold">{children}</h4>,
 h5:({children})=><h5 className="mb-1 mt-2 font-semibold">{children}</h5>,
 h6:({children})=><h6 className="mb-1 mt-2 font-semibold">{children}</h6>,
 p:({children})=><p className="mb-2 last:mb-0">{children}</p>,
 ul:({children})=><ul className="mb-2 list-disc space-y-1 pl-5 last:mb-0">{children}</ul>,
 ol:({children,start})=><ol start={start} className="mb-2 list-decimal space-y-1 pl-5 last:mb-0">{children}</ol>,
 li:({children})=><li className="pl-0.5 [&>p]:mb-1">{children}</li>,
 blockquote:({children})=><blockquote className="mb-2 border-l-2 border-muted-foreground/40 pl-3 text-muted-foreground">{children}</blockquote>,
 pre:({children})=><pre className="mb-2 max-w-full overflow-x-auto rounded-md border bg-muted/60 p-3 text-[11px] leading-relaxed [&_code]:block [&_code]:bg-transparent [&_code]:p-0 [&_code]:whitespace-pre">{children}</pre>,
 code:({children,className})=><code className={`rounded bg-muted px-1 py-0.5 font-mono text-[0.95em] ${className??''}`}>{children}</code>,
 a:({href,children,title})=><a href={href} title={title} target={href&&/^(?:https?:)?\/\//i.test(href)?'_blank':undefined} rel="noopener noreferrer" className="break-words text-primary underline underline-offset-2">{children}</a>,
 // Model-generated remote images must not issue implicit browser requests.
 img:({alt})=><span className="text-muted-foreground">{alt?`[图片：${alt}]`:'[图片]'}</span>,
 table:({children})=><div className="mb-2 max-w-full overflow-x-auto"><table className="w-full border-collapse text-left text-[11px]">{children}</table></div>,
 th:({children})=><th className="border bg-muted/50 px-2 py-1 font-medium">{children}</th>,
 td:({children})=><td className="border px-2 py-1 align-top">{children}</td>,
 hr:()=><hr className="my-3 border-border"/>,
 input:({checked})=><input type="checkbox" checked={checked??false} disabled readOnly className="mr-1 h-3 w-3 align-middle"/>,
};
/** Render only public assistant prose. Proposals remain literal editor source. */
export function AssistantMarkdown({text}:{text:string}){return <div className="min-w-0 whitespace-normal break-words text-foreground"><ReactMarkdown remarkPlugins={[remarkGfm]} components={components} skipHtml>{text}</ReactMarkdown></div>;}
