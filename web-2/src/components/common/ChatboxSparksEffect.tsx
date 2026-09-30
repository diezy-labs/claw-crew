import React from 'react';
import { SparksConfig } from '../../utils/sparksEngine';

export interface ChatboxSparksEffectProps {
  config: SparksConfig;
  children: React.ReactNode;
  className?: string;
}

export const ChatboxSparksEffect: React.FC<ChatboxSparksEffectProps> = ({
  config,
  children,
  className = ''
}) => {
  if (!config.enabled) {
    return <div className={`relative ${className}`}>{children}</div>;
  }

  const { style, color, intensity } = config;

  // Opacity multipliers
  const opacityMap = {
    subtle: 0.35,
    balanced: 0.65,
    vivid: 0.95
  };
  const baseOpacity = opacityMap[intensity] || 0.65;

  return (
    <div className={`relative group ${className}`}>
      {/* 1. Cyber Neon Sweep Border */}
      {style === 'cyber-neon' && (
        <div
          className="absolute -inset-[1.5px] rounded-xl pointer-events-none transition-all duration-300 animate-pulse"
          style={{
            background: `linear-gradient(90deg, ${color}00, ${color}cc, ${color}00)`,
            filter: `blur(${intensity === 'vivid' ? '6px' : '3px'})`,
            opacity: baseOpacity
          }}
        />
      )}

      {/* 2. Sovereign Breathing Aura */}
      {style === 'sovereign-aura' && (
        <div
          className="absolute -inset-1 rounded-2xl pointer-events-none transition-all duration-700 animate-pulse"
          style={{
            boxShadow: `0 0 ${intensity === 'vivid' ? '24px' : '14px'} ${color}`,
            opacity: baseOpacity * 0.75
          }}
        />
      )}

      {/* 3. Constellation Sparks (Glittering Star Dots) */}
      {style === 'constellation' && (
        <div className="absolute inset-0 pointer-events-none overflow-visible">
          {[
            { top: '-4px', left: '15%', size: 4, delay: '0s', dur: '2s' },
            { top: '-6px', left: '75%', size: 5, delay: '0.7s', dur: '2.4s' },
            { bottom: '-4px', left: '30%', size: 3.5, delay: '1.2s', dur: '1.8s' },
            { bottom: '-5px', left: '85%', size: 4.5, delay: '0.4s', dur: '2.2s' },
            { top: '35%', left: '-5px', size: 3, delay: '1.5s', dur: '2.5s' },
            { top: '65%', right: '-5px', size: 4, delay: '0.9s', dur: '2.1s' }
          ].map((spark, idx) => (
            <span
              key={idx}
              className="absolute rounded-full animate-ping"
              style={{
                top: spark.top,
                bottom: spark.bottom,
                left: spark.left,
                right: spark.right,
                width: `${spark.size}px`,
                height: `${spark.size}px`,
                backgroundColor: color,
                boxShadow: `0 0 8px ${color}`,
                animationDuration: spark.dur,
                animationDelay: spark.delay,
                opacity: baseOpacity
              }}
            />
          ))}
          {/* Subtle perimeter border glow */}
          <div
            className="absolute -inset-[1px] rounded-xl pointer-events-none"
            style={{
              boxShadow: `0 0 10px ${color}55`,
              opacity: baseOpacity
            }}
          />
        </div>
      )}

      {/* 4. Pirate Cannon Embers (Rising Sparks) */}
      {style === 'pirate-embers' && (
        <div className="absolute inset-0 pointer-events-none overflow-visible">
          {[
            { left: '20%', size: 3, delay: '0.2s', dur: '1.7s' },
            { left: '45%', size: 4, delay: '0.8s', dur: '2.1s' },
            { left: '70%', size: 3.5, delay: '0.5s', dur: '1.9s' },
            { left: '90%', size: 2.5, delay: '1.3s', dur: '1.5s' }
          ].map((ember, idx) => (
            <span
              key={idx}
              className="absolute rounded-full animate-bounce"
              style={{
                bottom: '-2px',
                left: ember.left,
                width: `${ember.size}px`,
                height: `${ember.size}px`,
                backgroundColor: color,
                boxShadow: `0 0 6px ${color}`,
                animationDuration: ember.dur,
                animationDelay: ember.delay,
                opacity: baseOpacity
              }}
            />
          ))}
          <div
            className="absolute -inset-[1px] rounded-xl pointer-events-none"
            style={{
              boxShadow: `0 0 12px ${color}66`,
              opacity: baseOpacity * 0.8
            }}
          />
        </div>
      )}

      {/* The Actual Content (Input Box / Composer Bar) */}
      <div className="relative z-10">{children}</div>
    </div>
  );
};
