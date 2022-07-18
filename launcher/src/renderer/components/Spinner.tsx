import React from "react";

export default function Spinner({
  size,
  style,
}: {
  size?: string;
  style?: React.CSSProperties;
}) {
  return (
    <img
      src={require("../../../static/images/spinner.gif")}
      style={{
        ...style,
        width: size,
        height: size,
        userSelect: "none",
        pointerEvents: "none",
      }}
    />
  );
}
