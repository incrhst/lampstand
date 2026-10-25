import React from "react";
import { useTheme } from "@video/core";
import { roleColors } from "../theme";
import { roleInfo, RoleKey } from "../roles";

export const Pill: React.FC<{ role: RoleKey; size?: number; style?: React.CSSProperties }> = ({ role, size = 30, style }) => {
  const t = useTheme();
  return (
    <div style={{ background: roleColors[role], color: "#fff", fontFamily: t.fonts.label, fontWeight: 700, fontSize: size, letterSpacing: "0.06em", padding: `${size * 0.28}px ${size * 0.7}px`, borderRadius: size * 2, whiteSpace: "nowrap", ...style }}>
      {roleInfo[role].name}
    </div>
  );
};
