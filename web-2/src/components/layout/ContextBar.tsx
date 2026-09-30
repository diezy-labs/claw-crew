import React from 'react';
import { Navbar, NavbarProps } from '../common/Navbar';

export const ContextBar: React.FC<NavbarProps> = (props) => {
  return <Navbar {...props} />;
};
