import React from 'react';
import { Search, X } from 'lucide-react';

export interface SearchBarProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  onClear?: () => void;
  size?: 'xs' | 'sm' | 'md';
  autoFocus?: boolean;
  className?: string;
  inputClassName?: string;
  shortcut?: string; // e.g. '/' or '⌘K'
  disabled?: boolean;
}

export const SearchBar: React.FC<SearchBarProps> = ({
  value,
  onChange,
  placeholder = 'Search...',
  onClear,
  size = 'sm',
  autoFocus = false,
  className = '',
  inputClassName = '',
  shortcut,
  disabled = false
}) => {
  const sizeStyles = {
    xs: {
      container: 'h-7 text-xs',
      icon: 'w-3 h-3 left-2 text-neutral-400',
      input: 'pl-6 pr-6 text-xs',
      clearBtn: 'right-1.5 p-0.5'
    },
    sm: {
      container: 'h-8 text-xs',
      icon: 'w-3.5 h-3.5 left-2.5 text-neutral-400',
      input: 'pl-8 pr-7 text-xs',
      clearBtn: 'right-1.5 p-1'
    },
    md: {
      container: 'h-9.5 text-sm',
      icon: 'w-4 h-4 left-3 text-neutral-400',
      input: 'pl-9 pr-8 text-sm',
      clearBtn: 'right-2 p-1'
    }
  }[size];

  const handleClear = () => {
    onChange('');
    if (onClear) onClear();
  };

  return (
    <div className={`relative flex items-center min-w-0 ${sizeStyles.container} ${className}`}>
      <Search
        className={`absolute pointer-events-none transition-colors ${sizeStyles.icon}`}
      />
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoFocus={autoFocus}
        disabled={disabled}
        className={`w-full h-full rounded-lg border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/90 text-neutral-900 dark:text-neutral-100 placeholder:text-neutral-400 dark:placeholder:text-neutral-500 focus:outline-none focus:ring-1 focus:ring-teal-500/50 focus:border-teal-500/60 transition-all font-sans disabled:opacity-50 disabled:cursor-not-allowed shadow-2xs ${sizeStyles.input} ${inputClassName}`}
      />

      {value ? (
        <button
          type="button"
          onClick={handleClear}
          aria-label="Clear search"
          tabIndex={-1}
          className={`absolute text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 rounded-md hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer ${sizeStyles.clearBtn}`}
        >
          <X className="w-3 h-3" />
        </button>
      ) : shortcut ? (
        <span className="absolute right-2 pointer-events-none hidden sm:inline-flex items-center text-[10px] font-mono text-neutral-400 dark:text-neutral-500 bg-neutral-100 dark:bg-neutral-800 border border-neutral-200/80 dark:border-neutral-700/80 px-1.5 py-0.5 rounded">
          {shortcut}
        </span>
      ) : null}
    </div>
  );
};
