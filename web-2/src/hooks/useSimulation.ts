import { useEffect } from 'react';
import { useFleetStore } from '../store/fleetStore';

export const useSimulation = () => {
  const { simulateVoyageTick } = useFleetStore();

  useEffect(() => {
    // Tick every 8 seconds to advance running voyages smoothly
    const interval = setInterval(() => {
      simulateVoyageTick();
    }, 8000);

    return () => clearInterval(interval);
  }, [simulateVoyageTick]);
};
