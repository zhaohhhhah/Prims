import { useEffect, useId, useState } from 'react';

let signaturePlayed = false;
const SIGNATURE_SESSION_KEY = 'prims:auth-signature-seen';

function hasPlayedSignature(): boolean {
  if (signaturePlayed) return true;
  try { return sessionStorage.getItem(SIGNATURE_SESSION_KEY) === '1'; } catch { return false; }
}

/** Allura's licensed outlines, revealed by pen paths in writing order. */
export function PrimsSignature() {
  const maskId = useId().replace(/:/g, '');
  const [animate] = useState(() => !hasPlayedSignature());
  const [writing, setWriting] = useState(animate);
  const [greeting, setGreeting] = useState(0);

  useEffect(() => {
    signaturePlayed = true;
    try { sessionStorage.setItem(SIGNATURE_SESSION_KEY, '1'); } catch { /* Private mode still uses the module flag. */ }
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    if (motion.matches) { setWriting(false); return; }
    const finishTimer = window.setTimeout(() => setWriting(false), 1975);
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') setGreeting((value) => (value + 1) % 4);
    }, 6500);
    return () => { window.clearInterval(timer); window.clearTimeout(finishTimer); };
  }, []);

  const greetings = [
    { prefix: 'welcome to', suffix: '', lang: 'en' },
    { prefix: 'Bienvenue sur', suffix: '', lang: 'fr' },
    { prefix: '欢迎来到', suffix: '', lang: 'zh' },
    { prefix: '', suffix: 'へようこそ', lang: 'ja' },
  ];

  return (
    <div className={`prims-signature${animate ? ' prims-signature--intro' : ''}`} role="img" aria-label="Welcome to Prims">
      <div className="prims-greetings" aria-hidden="true">
        {greetings.map((phrase, index) => (
          <span className={`prims-greeting${greeting === index ? ' is-current' : ''}`} lang={phrase.lang} key={phrase.lang}>
            <span className="prims-greeting-prefix">{phrase.prefix}</span>
            <span className="prims-greeting-suffix">{phrase.suffix}</span>
          </span>
        ))}
      </div>
      <svg className="prims-signature-ink" viewBox="0 120 2010 720" aria-hidden="true" focusable="false">
        <defs>
          <mask id={maskId} maskUnits="userSpaceOnUse" x="-60" y="100" width="2160" height="760">
            <g fill="none" stroke="white" strokeLinecap="round" strokeLinejoin="round">
              <path className="prims-pen-stroke" pathLength="1" strokeWidth="73" d="M470 295 C430 250 350 330 312 408 C250 510 175 650 160 730 C150 782 206 735 245 683" style={{ animationDelay: '0ms', animationDuration: '285ms' }} />
              <path className="prims-pen-stroke" pathLength="1" strokeWidth="68" d="M120 466 C-5 426 68 307 200 246 C360 158 620 158 685 230 C815 380 525 550 192 537" style={{ animationDelay: '275ms', animationDuration: '450ms' }} />
              <path className="prims-pen-stroke" pathLength="1" strokeWidth="75" d="M557 625 C620 550 680 450 721 375 L673 456 L790 468 C825 490 700 620 680 698 C660 780 785 747 897 590" style={{ animationDelay: '715ms', animationDuration: '275ms' }} />
              <path className="prims-pen-stroke" pathLength="1" strokeWidth="73" d="M957 472 C940 530 895 630 886 693 C865 805 1000 722 1105 595" style={{ animationDelay: '980ms', animationDuration: '190ms' }} />
              <path className="prims-pen-stroke" pathLength="1" strokeWidth="74" d="M1175 475 L1098 724 C1170 645 1285 497 1365 478 C1430 456 1350 574 1290 669 C1335 624 1430 531 1510 521 C1480 578 1420 711 1468 730 C1520 780 1620 655 1674 595" style={{ animationDelay: '1160ms', animationDuration: '375ms' }} />
              <path className="prims-pen-stroke prims-pen-stroke--finish" pathLength="1" strokeWidth="65" d="M1930 474 C2050 354 1885 337 1760 435 C1690 490 1680 531 1755 568 C1900 639 1840 711 1745 762 C1670 806 1575 803 1678 722" style={{ animationDelay: '1525ms', animationDuration: '430ms' }} />
              <path className="prims-pen-stroke" pathLength="1" strokeWidth="66" d="M1018 383 L1055 328" style={{ animationDelay: '1145ms', animationDuration: '60ms' }} />
            </g>
          </mask>
        </defs>
        <g className="prims-letterforms" mask={writing ? `url(#${maskId})` : undefined}>
        <path transform="translate(0 750) scale(1 -1)" d="M169 -27Q167 -27 157.5 -25.0Q148 -23 139.5 -20.0Q131 -17 130 -13Q128 6 140.0 41.0Q152 76 172.5 119.5Q193 163 215 205Q211 205 206.5 205.5Q202 206 197 206Q185 208 185 217Q185 222 196 222Q204 222 210.0 221.5Q216 221 223 221Q243 259 260.0 289.5Q277 320 284 336Q290 347 292.0 354.5Q294 362 294 366Q294 372 292 376Q294 388 306.5 408.0Q319 428 338.0 448.0Q357 468 379.0 481.5Q401 495 420 495Q426 495 438.5 491.5Q451 488 463.0 481.0Q475 474 478 461Q479 459 479 455Q479 452 476 452Q474 452 470.5 453.5Q467 455 465 455Q443 455 417.5 434.5Q392 414 365.0 380.0Q338 346 311.5 304.5Q285 263 261 221Q311 223 367.0 233.5Q423 244 478.0 262.0Q533 280 580.0 306.0Q627 332 659 365Q694 402 694 441Q694 470 672.0 495.0Q650 520 603 533Q575 541 539.5 545.0Q504 549 463 549Q399 549 331.5 538.0Q264 527 205.0 504.5Q146 482 106.0 448.0Q66 414 58 367Q55 343 67.0 329.0Q79 315 96.5 307.5Q114 300 124 294Q131 288 131 284Q131 277 118 279Q104 282 83.5 293.0Q63 304 47.5 323.5Q32 343 32 369Q32 373 32.5 377.0Q33 381 34 385Q34 386 33.5 391.5Q33 397 33 401Q39 432 63.5 459.5Q88 487 119.5 508.5Q151 530 178 543Q225 566 291.0 579.5Q357 593 429 593Q473 593 516.5 587.0Q560 581 601 568Q658 549 689.5 509.5Q721 470 718 427Q715 386 689.5 353.0Q664 320 624.5 295.5Q585 271 537.5 253.5Q490 236 442 225Q396 215 344.5 210.0Q293 205 252 204Q228 161 209.5 123.0Q191 85 182 62Q174 40 174 31Q174 17 185 17Q199 17 215 36Q226 49 235.5 60.5Q245 72 252 70Q253 69 253 67Q253 62 246.0 53.5Q239 45 238 44Q231 34 219.5 17.0Q208 0 195.5 -13.5Q183 -27 169 -27Z" />
        <path transform="translate(523 750) scale(1 -1)" d="M172 -12Q155 -12 142.0 -4.5Q129 3 124 24Q121 35 121 46Q121 76 136.5 104.5Q152 133 173.0 157.5Q194 182 209 199Q217 207 227.5 220.0Q238 233 238 246Q238 254 230.0 261.5Q222 269 201 272Q189 274 178.5 275.5Q168 277 157 277Q129 239 102.5 200.5Q76 162 46 125Q39 118 34 118Q28 118 28 124Q28 130 34 136Q88 202 127 277Q122 277 112.0 277.5Q102 278 96 279Q89 281 85.0 285.0Q81 289 81 293Q81 299 86 300Q99 304 114.0 312.0Q129 320 142 333Q156 347 168.0 362.5Q180 378 187 386Q194 391 202 391Q207 391 211.5 388.0Q216 385 216 379Q216 372 210 361Q201 347 190.5 330.0Q180 313 171 299Q189 298 207.0 297.0Q225 296 244 296Q254 296 263.5 296.5Q273 297 283 297Q313 297 313 282Q313 272 302.5 257.5Q292 243 277 226Q243 187 219.0 144.0Q195 101 187 78Q179 54 179 42Q179 18 198 18Q215 18 239.5 35.0Q264 52 290.0 76.5Q316 101 337.0 124.5Q358 148 367 160Q369 163 373 163Q377 163 380.0 159.5Q383 156 380 151Q369 134 348.0 108.5Q327 83 301.0 58.0Q275 33 250 15Q237 5 214.5 -3.5Q192 -12 172 -12Z" />
        <path transform="translate(822 750) scale(1 -1)" d="M198 348Q185 348 171.0 350.5Q157 353 157 369Q157 372 170.5 383.0Q184 394 198.5 405.5Q213 417 216 419Q233 433 237 433Q242 433 237.0 420.0Q232 407 232 397Q232 392 226.0 380.0Q220 368 212.0 358.0Q204 348 198 348ZM89 -10Q71 -10 56.5 -2.0Q42 6 35 28Q34 31 33.5 34.0Q33 37 33 41Q33 61 42.0 91.0Q51 121 64 149Q58 141 50.0 129.5Q42 118 27 100Q23 96 18 96Q11 96 11 104Q11 110 15 114Q41 147 61.5 173.0Q82 199 92 217Q92 218 91.5 218.5Q91 219 91 220Q91 223 94 229Q102 253 110.5 269.5Q119 286 129 288Q131 288 133.0 288.5Q135 289 137 289Q148 289 157.0 284.0Q166 279 166 269Q166 260 159.5 247.0Q153 234 143 215Q121 169 103.5 131.5Q86 94 84 70Q83 66 83.0 62.5Q83 59 83 55Q83 15 109 15Q125 15 150.0 32.5Q175 50 201.0 75.5Q227 101 248.5 124.5Q270 148 278 160Q281 163 284 163Q288 163 290.5 160.0Q293 157 291 152Q282 135 260.5 109.5Q239 84 212.5 58.5Q186 33 161 15Q148 5 128.5 -2.5Q109 -10 89 -10Z" />
        <path transform="translate(1033 750) scale(1 -1)" d="M23 -13Q15 -13 18 -6Q22 1 32.5 29.5Q43 58 56.5 97.0Q70 136 83 172Q73 157 61.5 143.0Q50 129 42 120Q39 117 38 117Q35 117 35 123Q35 135 41 145Q62 172 73.0 187.5Q84 203 91.5 215.5Q99 228 107 244Q109 255 114.5 271.0Q120 287 141 287Q172 287 172 274Q172 270 170.0 265.5Q168 261 166 256Q154 229 140.0 194.0Q126 159 114.5 128.0Q103 97 97 80Q108 94 130.0 119.5Q152 145 180.5 175.5Q209 206 239.5 233.5Q270 261 297.0 279.0Q324 297 343 297Q366 297 366 271Q366 251 350 219Q343 204 331.0 178.5Q319 153 312 140Q330 156 353.0 175.5Q376 195 401.5 212.0Q427 229 449 237Q454 239 469.0 243.5Q484 248 495 248Q510 248 510 238Q510 231 503 223Q486 197 472.5 167.5Q459 138 451.0 112.0Q443 86 441 70Q440 66 440.0 62.5Q440 59 440 55Q440 15 466 15Q482 15 507.0 32.5Q532 50 558.0 75.5Q584 101 605.5 124.5Q627 148 635 160Q638 163 641 163Q645 163 647.5 160.0Q650 157 648 152Q639 135 617.5 109.5Q596 84 569.5 58.5Q543 33 518 15Q505 5 485.5 -2.5Q466 -10 446 -10Q428 -10 413.5 -2.0Q399 6 392 28Q389 37 389 51Q389 73 395.0 101.0Q401 129 408 148Q413 162 421.5 179.5Q430 197 437 209Q406 197 377.0 173.0Q348 149 324.5 124.5Q301 100 285 84Q281 73 269.5 66.0Q258 59 248 59Q233 59 233 72Q233 77 236 84Q246 108 258.5 133.5Q271 159 281.5 180.0Q292 201 296 209Q304 223 304 231Q304 239 297 239Q290 239 274.0 230.0Q258 221 232 198Q211 179 186.0 148.5Q161 118 138.0 86.0Q115 54 97.5 31.0Q80 8 73 4Q67 1 49.5 -6.0Q32 -13 23 -13Z" />
        <path transform="translate(1601 750) scale(1 -1)" d="M51 -50Q11 -50 11 -28Q11 -12 28 5Q41 18 57.0 26.0Q73 34 77 34Q82 34 82.5 30.0Q83 26 78 24Q68 18 55.5 7.5Q43 -3 43 -15Q43 -31 62 -31Q85 -31 110.0 -17.5Q135 -4 156.5 17.0Q178 38 191.5 59.5Q205 81 205 97Q205 110 196.0 121.0Q187 132 159 147Q140 158 122.5 170.0Q105 182 96 199Q88 185 75.5 165.5Q63 146 46 125Q40 119 34 119Q28 119 28 124Q28 127 33 134Q60 166 73.0 193.0Q86 220 91 234Q95 263 118.0 289.0Q141 315 174.0 334.5Q207 354 241.5 365.0Q276 376 303 376Q330 376 351.0 366.0Q372 356 372 334Q372 322 365 308Q356 289 335 274Q334 273 332 273Q324 273 324 280Q324 285 329 290Q337 297 342.0 307.0Q347 317 347 326Q347 344 326 344Q304 344 268.5 332.0Q233 320 203 301Q169 279 146.0 253.0Q123 227 123 207Q123 197 129.5 188.5Q136 180 155 169Q165 163 183.0 153.5Q201 144 214 136Q255 111 255 83Q255 60 235.0 36.5Q215 13 183.5 -6.5Q152 -26 117.0 -38.0Q82 -50 51 -50Z" />
        </g>
      </svg>
    </div>
  );
}
