import React from "react";
import { Trans } from "react-i18next";

import { clipboard } from "@electron/remote";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import Button, { ButtonProps } from "@mui/material/Button";
import IconButton, { IconButtonProps } from "@mui/material/IconButton";
import Tooltip, { TooltipProps } from "@mui/material/Tooltip";

export default function CopyButton({
  value,
  disabled,
  TooltipProps,
  ...props
}: {
  value: string;
  disabled?: boolean;
  TooltipProps?: Partial<TooltipProps>;
} & IconButtonProps) {
  const [currentTimeout, setCurrentTimeout] =
    React.useState<NodeJS.Timeout | null>(null);
  return (
    <Tooltip
      title={
        currentTimeout != null ? (
          <Trans i18nKey="common:copied-to-clipboard" />
        ) : (
          <Trans i18nKey="common:copy-to-clipboard" />
        )
      }
      {...TooltipProps}
    >
      <IconButton
        onClick={() => {
          clipboard.writeText(value);
          if (currentTimeout != null) {
            clearTimeout(currentTimeout);
          }
          setCurrentTimeout(
            setTimeout(() => {
              setCurrentTimeout(null);
            }, 1000)
          );
        }}
        edge="end"
        disabled={disabled}
        {...props}
      >
        <ContentCopyIcon />
      </IconButton>
    </Tooltip>
  );
}

export function CopyButtonWithLabel({
  value,
  disabled,
  TooltipProps,
  ...props
}: {
  value: string;
  disabled?: boolean;
  TooltipProps?: Partial<TooltipProps>;
} & ButtonProps) {
  const [currentTimeout, setCurrentTimeout] =
    React.useState<NodeJS.Timeout | null>(null);
  return (
    <Tooltip
      open={currentTimeout != null}
      title={<Trans i18nKey="common:copied-to-clipboard" />}
      {...TooltipProps}
    >
      <Button
        onClick={() => {
          clipboard.writeText(value);
          if (currentTimeout != null) {
            clearTimeout(currentTimeout);
          }
          setCurrentTimeout(
            setTimeout(() => {
              setCurrentTimeout(null);
            }, 1000)
          );
        }}
        startIcon={<ContentCopyIcon />}
        disabled={disabled}
        {...props}
      >
        <Trans i18nKey="common:copy-to-clipboard" />
      </Button>
    </Tooltip>
  );
}
