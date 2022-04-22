import AppBar from "@mui/material/AppBar";
import Stack from "@mui/material/Stack";
import Chip from "@mui/material/Chip";
import TextField from "@mui/material/TextField";
import Select from "@mui/material/Select";
import MenuItem from "@mui/material/MenuItem";
import Autocomplete from "@mui/material/Autocomplete";
import Box from "@mui/material/Box";
import CssBaseline from "@mui/material/CssBaseline";
import Toolbar from "@mui/material/Toolbar";
import Typography from "@mui/material/Typography";
import ThemeProvider from "@mui/system/ThemeProvider";
import React from "react";
import theme from "../theme";

import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import Paper from "@mui/material/Paper";

export default function App(): JSX.Element {
  return (
    <ThemeProvider theme={theme}>
      <Box sx={{ display: "flex" }}>
        <CssBaseline />
        <Box
          component="main"
          sx={{
            backgroundColor: (theme) =>
              theme.palette.mode === "light"
                ? theme.palette.grey[100]
                : theme.palette.grey[900],
            flexGrow: 1,
            height: "100vh",
            overflow: "auto",
          }}
        >
          <Box sx={{ px: 1, py: 1 }}>
            <Autocomplete
              disablePortal
              size="small"
              id="combo-box-demo"
              options={[{ label: "Cannon A", value: 0 }]}
              renderInput={(params) => (
                <TextField
                  {...params}
                  label="Add a chip (24 remaining)..."
                  variant="standard"
                />
              )}
            />

            <TableContainer sx={{ py: 1 }}>
              <Table size="small">
                <TableBody>
                  <TableRow>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/chipicons/s1.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell width="100%">
                      <strong>Cannon</strong>
                    </TableCell>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/elements/null.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell>
                      <Select size="small" variant="standard" value={0}>
                        <MenuItem value={0}>A</MenuItem>
                        <MenuItem value={1}>B</MenuItem>
                        <MenuItem value={2}>C</MenuItem>
                        <MenuItem value={26}>﹡</MenuItem>
                      </Select>
                    </TableCell>
                    <TableCell>
                      <span style={{ whiteSpace: "nowrap" }}>6 MB</span>
                    </TableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/chipicons/s1.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell width="100%">
                      <strong>Cannon</strong>
                    </TableCell>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/elements/null.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell>
                      <Select size="small" variant="standard" value={0}>
                        <MenuItem value={0}>A</MenuItem>
                        <MenuItem value={1}>B</MenuItem>
                        <MenuItem value={2}>C</MenuItem>
                        <MenuItem value={26}>﹡</MenuItem>
                      </Select>
                    </TableCell>
                    <TableCell>
                      <span style={{ whiteSpace: "nowrap" }}>6 MB</span>
                    </TableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/chipicons/s1.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell width="100%">
                      <strong>Cannon</strong>
                    </TableCell>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/elements/null.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell>
                      <Select size="small" variant="standard" value={0}>
                        <MenuItem value={0}>A</MenuItem>
                        <MenuItem value={1}>B</MenuItem>
                        <MenuItem value={2}>C</MenuItem>
                        <MenuItem value={26}>﹡</MenuItem>
                      </Select>
                    </TableCell>
                    <TableCell>
                      <span style={{ whiteSpace: "nowrap" }}>6 MB</span>
                    </TableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/chipicons/s1.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell width="100%">
                      <strong>Cannon</strong>
                    </TableCell>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/elements/null.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell>
                      <Select size="small" variant="standard" value={1}>
                        <MenuItem value={0}>A</MenuItem>
                        <MenuItem value={1}>B</MenuItem>
                        <MenuItem value={2}>C</MenuItem>
                        <MenuItem value={26}>﹡</MenuItem>
                      </Select>
                    </TableCell>
                    <TableCell>
                      <span style={{ whiteSpace: "nowrap" }}>6 MB</span>
                    </TableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/chipicons/s1.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell width="100%">
                      <strong>Cannon</strong>
                    </TableCell>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/elements/null.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell>
                      <Select size="small" variant="standard" value={1}>
                        <MenuItem value={0}>A</MenuItem>
                        <MenuItem value={1}>B</MenuItem>
                        <MenuItem value={2}>C</MenuItem>
                        <MenuItem value={26}>﹡</MenuItem>
                      </Select>
                    </TableCell>
                    <TableCell>
                      <span style={{ whiteSpace: "nowrap" }}>6 MB</span>
                    </TableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/chipicons/s1.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell width="100%">
                      <strong>Cannon</strong>
                    </TableCell>
                    <TableCell width="0px">
                      <img
                        src="https://bntools.murk.land/chiplibrary/images/6/elements/null.png"
                        width="28"
                        height="28"
                        style={{ imageRendering: "pixelated" }}
                      />
                    </TableCell>
                    <TableCell>
                      <Select size="small" variant="standard" value={1}>
                        <MenuItem value={0}>A</MenuItem>
                        <MenuItem value={1}>B</MenuItem>
                        <MenuItem value={2}>C</MenuItem>
                        <MenuItem value={26}>﹡</MenuItem>
                      </Select>
                    </TableCell>
                    <TableCell>
                      <span style={{ whiteSpace: "nowrap" }}>6 MB</span>
                    </TableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </TableContainer>
          </Box>
        </Box>
      </Box>
    </ThemeProvider>
  );
}
