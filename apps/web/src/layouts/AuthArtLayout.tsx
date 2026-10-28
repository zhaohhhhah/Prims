import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet } from 'react-router-dom';

import { advanceMonetCycle, createMonetCycle, currentMonetIndex, MONET_ART, nextMonetIndex, rememberMonetArtwork } from '../components/auth/monet-art';
import '../components/auth/auth-art.css';

function useReducedMotion() {
  const [reduced, setReduced] = useState(() => window.matchMedia('(prefers-reduced-motion: reduce)').matches);
  useEffect(() => {
    const query = window.matchMedia('(prefers-reduced-motion: reduce)');
    const update = () => setReduced(query.matches);
    query.addEventListener('change', update);
    return () => query.removeEventListener('change', update);
  }, []);
  return reduced;
}

/** Lives in the common route, so navigating between forms never replaces its images or clock. */
function AuthGallery() {
  const { i18n } = useTranslation();
  const reducedMotion = useReducedMotion();
  const [art, setArt] = useState(createMonetCycle);
  const artIndex = currentMonetIndex(art);
  const previousIndex = art.previous;
  const nextIndex = nextMonetIndex(art);
  const loadedImages = useRef(new Set<number>());

  useEffect(() => {
    if (reducedMotion) return;
    const timer = window.setInterval(() => {
      if (document.visibilityState !== 'visible') return;
      setArt((value) => loadedImages.current.has(nextMonetIndex(value)) ? advanceMonetCycle(value) : value);
    }, 10000);
    return () => window.clearInterval(timer);
  }, [reducedMotion]);

  useEffect(() => { rememberMonetArtwork(artIndex); }, [artIndex]);
  useEffect(() => {
    if (previousIndex === null) return;
    const timer = window.setTimeout(() => setArt((value) => ({ ...value, previous: null })), reducedMotion ? 0 : 1600);
    return () => window.clearTimeout(timer);
  }, [artIndex, previousIndex, reducedMotion]);

  const visibleArt = Array.from(new Set([...(previousIndex === null ? [] : [previousIndex]), artIndex, nextIndex]));
  return (
    <aside className="auth-art-gallery" aria-label={i18n.language.startsWith('zh') ? '莫奈画作' : 'Paintings by Claude Monet'}>
      <div className="auth-art-images" aria-hidden="true">
        {visibleArt.map((index) => (
          <img key={index} src={MONET_ART[index]} alt="" decoding="async"
            onLoad={() => { loadedImages.current.add(index); }}
            className={`auth-art-image${index === artIndex ? ' is-current' : index === previousIndex ? ' is-previous' : ' is-next'}`}
            style={{ objectPosition: index === 0 ? '48% 48%' : '50% 50%' }} />
        ))}
      </div>
    </aside>
  );
}

export function AuthArtLayout() {
  return <main className="auth-art-shell"><AuthGallery /><Outlet /></main>;
}
