import React, { useState } from 'react';
import { Image as ImageIcon } from 'lucide-react';

interface OptimizedImageProps extends React.ImgHTMLAttributes<HTMLImageElement> {
  src: string;
  alt: string;
  fallbackIcon?: React.ReactNode;
  aspectRatio?: '16/9' | '4/3' | '1/1' | '3/2';
}

export const OptimizedImage: React.FC<OptimizedImageProps> = ({
  src,
  alt,
  fallbackIcon,
  aspectRatio = '16/9',
  className = '',
  ...props
}) => {
  const [hasError, setHasError] = useState(false);
  const [isLoading, setIsLoading] = useState(true);

  const aspectClass =
    aspectRatio === '1/1'
      ? 'aspect-square'
      : aspectRatio === '4/3'
      ? 'aspect-4/3'
      : aspectRatio === '3/2'
      ? 'aspect-3/2'
      : 'aspect-video';

  if (hasError) {
    return (
      <div
        className={`w-full ${aspectClass} rounded-lg bg-neutral-100 dark:bg-neutral-800/80 border border-neutral-200 dark:border-neutral-700/60 flex flex-col items-center justify-center p-4 text-neutral-400 ${className}`}
      >
        {fallbackIcon || <ImageIcon className="w-6 h-6 opacity-40 mb-1" />}
        <span className="text-[11px] font-mono text-neutral-500 line-clamp-1">{alt}</span>
      </div>
    );
  }

  return (
    <div className={`relative overflow-hidden rounded-lg ${aspectClass} ${className}`}>
      {isLoading && (
        <div className="absolute inset-0 bg-neutral-200 dark:bg-neutral-800 animate-pulse" />
      )}
      <img
        src={src}
        alt={alt}
        loading="lazy"
        decoding="async"
        referrerPolicy="no-referrer"
        onLoad={() => setIsLoading(false)}
        onError={() => {
          setIsLoading(false);
          setHasError(true);
        }}
        className={`w-full h-full object-cover transition-opacity duration-300 ${
          isLoading ? 'opacity-0' : 'opacity-100'
        }`}
        {...props}
      />
    </div>
  );
};
