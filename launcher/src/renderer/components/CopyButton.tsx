import React from "react";
import { Trans } from "react-i18next";

import { clipboard } from "@electron/remote";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import IconButton, { IconButtonProps } from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";

export default function CopyButton({
  value,
  disabled,
  ...props
}: {
  value: string;
  disabled?: boolean;
} & IconButtonProps) {
  const [clicked, setClicked] = React.useState(false);
  return (
    <Tooltip
      title={
        clicked ? (
          <Trans i18nKey="common:copied-to-clipboard" />
        ) : (
          <Trans i18nKey="common:copy-to-clipboard" />
        )
      }
    >
      <IconButton
        onClick={() => {
          clipboard.writeText(value);
          setClicked(true);
          setTimeout(() => {
            setClicked(false);
          }, 1000);
        }}
        edge="end"
        disabled={disabled}
        {...props}
      >
        <ContentCopyIcon fontSize="small" />
      </IconButton>
    </Tooltip>
  );
}
