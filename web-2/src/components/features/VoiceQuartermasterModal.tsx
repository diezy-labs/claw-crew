import React, { useState, useEffect, useRef } from 'react';
import {
  Mic,
  MicOff,
  Volume2,
  Square,
  Radio,
  Sparkles,
  Compass,
  X,
  PhoneOff,
  Zap,
  RotateCcw
} from 'lucide-react';
import { useFleetStore } from '../../store/fleetStore';

interface VoiceQuartermasterModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSendTranscript?: (text: string) => void;
}

type VadState = 'idle' | 'listening' | 'thinking' | 'speaking';

export const VoiceQuartermasterModal: React.FC<VoiceQuartermasterModalProps> = ({
  isOpen,
  onClose,
  onSendTranscript
}) => {
  const { sendQuartermasterMessage, selectedProject, selectedWorkspace } = useFleetStore();
  const [vadState, setVadState] = useState<VadState>('listening');
  const [isMuted, setIsMuted] = useState(false);
  const [transcript, setTranscript] = useState<Array<{ sender: 'user' | 'qm'; text: string; time: string }>>([
    {
      sender: 'qm',
      text: 'Good evening, Pirate King. Voice link established with Quartermaster. How shall we direct the Fleet today?',
      time: 'Just now'
    }
  ]);
  const [currentSpokenText, setCurrentSpokenText] = useState('');
  const [audioLevels, setAudioLevels] = useState<number[]>([15, 30, 60, 80, 50, 25, 45, 70, 35, 20]);

  // Audio waveform animation simulation based on state
  useEffect(() => {
    if (!isOpen) return;

    const interval = setInterval(() => {
      if (vadState === 'listening' || vadState === 'speaking') {
        setAudioLevels((prev) =>
          prev.map(() => Math.floor(Math.random() * (vadState === 'speaking' ? 85 : 45) + 15))
        );
      } else {
        setAudioLevels([10, 10, 10, 10, 10, 10, 10, 10, 10, 10]);
      }
    }, 120);

    return () => clearInterval(interval);
  }, [isOpen, vadState]);

  // Simulated voice interaction cycle
  const handleSimulateUserSpeech = (prompt: string) => {
    if (vadState === 'speaking') {
      // Barge-in interruption
      handleBargeIn();
    }

    setVadState('listening');
    setCurrentSpokenText(prompt);

    setTimeout(() => {
      setTranscript((prev) => [
        ...prev,
        { sender: 'user', text: prompt, time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) }
      ]);
      setCurrentSpokenText('');
      setVadState('thinking');

      setTimeout(() => {
        setVadState('speaking');
        const reply =
          prompt.includes('release') || prompt.includes('v1.4')
            ? 'I have verified Developer Ship’s progress on the release candidate. The QA Reviewer isolated the CI teardown issue. We have 1 pending Captain Approval before tagging v1.4.'
            : prompt.includes('quest')
            ? 'Forging new Quest with high priority. Routing execution map to Horizon Navigator on Developer Ship.'
            : `Acknowledged, Pirate King. Standing by across ${selectedWorkspace} / ${selectedProject}. All Fleet systems are operating within your defined Treasury and Fleet Code caps.`;

        setTranscript((prev) => [
          ...prev,
          { sender: 'qm', text: reply, time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) }
        ]);

        if (onSendTranscript) {
          onSendTranscript(`[Voice Command]: ${prompt}`);
        }

        setTimeout(() => {
          setVadState('listening');
        }, 3200);
      }, 1100);
    }, 1500);
  };

  const handleBargeIn = () => {
    // Interruption / barge-in feature from voice_duplex.rs
    setVadState('listening');
  };

  if (!isOpen) return null;

  return (
    <div
      onClick={onClose}
      className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-4 bg-black/75 backdrop-blur-md animate-in fade-in duration-200 cursor-pointer"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-xl rounded-3xl border border-teal-500/30 bg-[#121417] shadow-2xl p-6 space-y-6 cursor-default text-neutral-100 relative overflow-hidden"
      >
        {/* Ambient oceanic background glow */}
        <div className="absolute -top-32 -left-32 w-64 h-64 rounded-full bg-teal-500/15 blur-3xl pointer-events-none" />
        <div className="absolute -bottom-32 -right-32 w-64 h-64 rounded-full bg-cyan-500/15 blur-3xl pointer-events-none" />

        {/* Header without X (backdrop closes) */}
        <div className="flex items-center justify-between border-b border-neutral-800/80 pb-3">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-lg bg-teal-500/20 text-teal-400 flex items-center justify-center font-bold border border-teal-500/30">
              <Radio className="w-4 h-4 animate-pulse text-teal-400" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h3 className="text-sm font-bold tracking-tight text-white">
                  Voice Quartermaster · Duplex Audio Link
                </h3>
                <span className="text-[9px] font-mono px-1.5 py-0.2 rounded bg-teal-500/20 text-teal-300 font-semibold border border-teal-500/30">
                  Silero VAD v4.0
                </span>
              </div>
              <div className="text-[11px] text-neutral-400">
                Direct voice command bridge · Full-duplex PCM stream with barge-in support
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <span
              className={`text-[10px] font-mono font-bold px-2 py-0.5 rounded-full flex items-center gap-1.5 ${
                vadState === 'speaking'
                  ? 'bg-cyan-500/20 text-cyan-300 border border-cyan-500/40 animate-pulse'
                  : vadState === 'listening'
                  ? 'bg-emerald-500/20 text-emerald-400 border border-emerald-500/40'
                  : vadState === 'thinking'
                  ? 'bg-amber-500/20 text-amber-400 border border-amber-500/40 animate-pulse'
                  : 'bg-neutral-800 text-neutral-400'
              }`}
            >
              <span className="w-1.5 h-1.5 rounded-full bg-current" />
              <span className="capitalize">{vadState}</span>
            </span>
          </div>
        </div>

        {/* Central Voice Orb & Waveform */}
        <div className="flex flex-col items-center justify-center py-6 space-y-5">
          <div className="relative flex items-center justify-center">
            {/* Outer animated halo pulses */}
            <div
              className={`absolute rounded-full transition-all duration-700 ${
                vadState === 'speaking'
                  ? 'w-44 h-44 bg-cyan-500/20 animate-ping'
                  : vadState === 'listening'
                  ? 'w-40 h-40 bg-teal-500/15 animate-pulse'
                  : 'w-36 h-36 bg-amber-500/10'
              }`}
            />
            <div
              className={`w-28 h-28 rounded-full flex items-center justify-center border-2 transition-all duration-500 shadow-2xl relative z-10 ${
                vadState === 'speaking'
                  ? 'bg-gradient-to-tr from-cyan-600 to-teal-400 border-cyan-300 shadow-[0_0_35px_rgba(6,182,212,0.6)] scale-105'
                  : vadState === 'listening'
                  ? 'bg-gradient-to-tr from-teal-600 to-emerald-500 border-teal-300 shadow-[0_0_25px_rgba(20,184,166,0.5)]'
                  : 'bg-gradient-to-tr from-amber-600 to-yellow-500 border-amber-300 shadow-[0_0_25px_rgba(245,158,11,0.5)]'
              }`}
            >
              <Compass className={`w-12 h-12 text-white ${vadState === 'thinking' ? 'animate-spin' : ''}`} />
            </div>
          </div>

          {/* Dynamic Audio Visualizer Bars */}
          <div className="flex items-center gap-1.5 h-10 px-4 py-1 rounded-full bg-neutral-900/80 border border-neutral-800">
            {audioLevels.map((lvl, i) => (
              <span
                key={i}
                style={{ height: `${lvl}%` }}
                className={`w-1 rounded-full transition-all duration-100 ${
                  vadState === 'speaking'
                    ? 'bg-cyan-400 shadow-[0_0_6px_rgba(6,182,212,0.8)]'
                    : vadState === 'listening'
                    ? 'bg-teal-400 shadow-[0_0_6px_rgba(20,184,166,0.8)]'
                    : 'bg-neutral-600'
                }`}
              />
            ))}
          </div>

          <div className="text-center space-y-1">
            <div className="text-xs font-medium text-neutral-300">
              {vadState === 'speaking'
                ? 'Quartermaster is speaking (Click “Barge-In” or speak to interrupt)'
                : vadState === 'listening'
                ? 'Listening for Captain’s voice command...'
                : vadState === 'thinking'
                ? 'Consulting Fleet Code & routing mission...'
                : 'Standby'}
            </div>
            {currentSpokenText && (
              <div className="text-xs font-mono text-teal-300 italic animate-pulse">
                &ldquo;{currentSpokenText}&rdquo;
              </div>
            )}
          </div>
        </div>

        {/* Live Conversation Transcript Stream */}
        <div className="space-y-2">
          <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider">
            Live Voice Transcript
          </span>
          <div className="max-h-36 overflow-y-auto space-y-2 p-3 rounded-xl bg-neutral-950/80 border border-neutral-800/80 text-xs scrollbar-none">
            {transcript.map((msg, idx) => (
              <div
                key={idx}
                className={`flex gap-2.5 ${msg.sender === 'user' ? 'justify-end' : 'justify-start'}`}
              >
                <div
                  className={`p-2.5 rounded-xl max-w-[85%] leading-relaxed ${
                    msg.sender === 'user'
                      ? 'bg-teal-600 text-white font-medium ml-auto'
                      : 'bg-neutral-800/80 text-neutral-200 border border-neutral-700/60'
                  }`}
                >
                  <div className="text-[9px] font-mono text-neutral-400 mb-0.5">
                    {msg.sender === 'user' ? 'You (Pirate King)' : 'Quartermaster'} &middot; {msg.time}
                  </div>
                  <div>{msg.text}</div>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Sample Voice Prompts for Quick Testing */}
        <div className="space-y-1.5">
          <span className="text-[10px] font-semibold text-neutral-400 uppercase tracking-wider">
            Quick Spoken Invocations
          </span>
          <div className="flex flex-wrap gap-1.5">
            <button
              onClick={() => handleSimulateUserSpeech('What is our release readiness for v1.4?')}
              className="px-2.5 py-1 rounded-lg text-[11px] bg-neutral-800 hover:bg-neutral-700 text-neutral-200 border border-neutral-700 transition-colors cursor-pointer"
            >
              &ldquo;What is our release readiness for v1.4?&rdquo;
            </button>
            <button
              onClick={() => handleSimulateUserSpeech('Create a new Quest to investigate socket teardown')}
              className="px-2.5 py-1 rounded-lg text-[11px] bg-neutral-800 hover:bg-neutral-700 text-neutral-200 border border-neutral-700 transition-colors cursor-pointer"
            >
              &ldquo;Create Quest for socket teardown&rdquo;
            </button>
            <button
              onClick={() => handleSimulateUserSpeech('Give me a Treasury and Ship health briefing')}
              className="px-2.5 py-1 rounded-lg text-[11px] bg-neutral-800 hover:bg-neutral-700 text-neutral-200 border border-neutral-700 transition-colors cursor-pointer"
            >
              &ldquo;Treasury &amp; Ship health briefing&rdquo;
            </button>
          </div>
        </div>

        {/* Bottom Voice Controls */}
        <div className="flex items-center justify-between pt-3 border-t border-neutral-800/80">
          <div className="flex items-center gap-2">
            <button
              onClick={() => setIsMuted(!isMuted)}
              className={`p-2 rounded-xl border text-xs font-medium transition-colors cursor-pointer flex items-center gap-1.5 ${
                isMuted
                  ? 'border-rose-500 bg-rose-500/20 text-rose-300'
                  : 'border-neutral-800 bg-neutral-900 text-neutral-300 hover:border-neutral-700'
              }`}
              title={isMuted ? 'Unmute microphone' : 'Mute microphone'}
            >
              {isMuted ? <MicOff className="w-3.5 h-3.5 text-rose-400" /> : <Mic className="w-3.5 h-3.5 text-teal-400" />}
              <span className="hidden sm:inline">{isMuted ? 'Muted' : 'Mic Active'}</span>
              <span className="sm:hidden">{isMuted ? 'Muted' : 'Active'}</span>
            </button>

            {vadState === 'speaking' && (
              <button
                onClick={handleBargeIn}
                className="px-3 py-1.5 rounded-xl border border-cyan-500/50 bg-cyan-500/20 text-cyan-300 text-xs font-semibold hover:bg-cyan-500/30 transition-colors flex items-center gap-1.5 cursor-pointer shadow-xs animate-pulse"
                title="Barge-In (Interrupt AI speech)"
              >
                <Zap className="w-3.5 h-3.5" />
                <span className="hidden sm:inline">Barge-In (Interrupt)</span>
                <span className="sm:hidden">Interrupt</span>
              </button>
            )}
          </div>

          <button
            onClick={onClose}
            className="px-4 py-2 rounded-xl bg-rose-600/90 hover:bg-rose-600 text-white font-semibold text-xs transition-colors flex items-center gap-1.5 cursor-pointer shadow-md"
          >
            <PhoneOff className="w-3.5 h-3.5" />
            <span>End Voice Link</span>
          </button>
        </div>
      </div>
    </div>
  );
};
