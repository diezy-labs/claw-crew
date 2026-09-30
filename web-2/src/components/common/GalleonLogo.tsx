import React from 'react';

interface GalleonLogoProps {
  className?: string;
  size?: number | string;
}

export const GalleonLogo: React.FC<GalleonLogoProps> = ({
  className = 'w-8 h-8',
  size
}) => {
  return (
    <svg
      viewBox="0 0 1000 1000"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      style={size ? { width: size, height: size } : undefined}
      aria-label="Galleon Fleet Logo"
    >
      {/* Dark rounded square base (no white outline) */}
      <rect width="1000" height="1000" rx="200" fill="#111215" />

      {/* Left Chevron Wing - Pale Cream Ivory */}
      <path
        d="M 200 503 L 450 535 L 355 780 L 348 595 Z"
        fill="#F4EFE6"
      />

      {/* Middle Chevron Wing - Warm Champagne Cream */}
      <path
        d="M 292 427 L 615 412 L 484 705 L 490 487 Z"
        fill="#EDE3D5"
      />

      {/* Top/Right Chevron Wing - Golden Bronze / Amber */}
      <path
        d="M 405 332 L 802 288 L 625 626 L 646 393 Z"
        fill="#C9A050"
      />
    </svg>
  );
};
