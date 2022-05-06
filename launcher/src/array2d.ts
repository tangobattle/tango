interface Array2D<T> extends Array<T> {
  nrows: number;
  ncols: number;
}

export default function array2d<T>(nrows: number, ncols: number): Array2D<T> {
  const arr2d = new Array<T>(nrows * ncols) as Array2D<T>;
  arr2d.nrows = nrows;
  arr2d.ncols = ncols;
  return arr2d;
}

array2d.from = function <T>(data: Array<T>, nrows: number, ncols: number) {
  const arr2d = [...data] as Array2D<T>;
  arr2d.nrows = nrows;
  arr2d.ncols = ncols;
  return arr2d;
};

array2d.full = function <T>(v: T, nrows: number, ncols: number) {
  const arr2d = array2d<T>(nrows, ncols);
  arr2d.fill(v, 0, nrows * ncols);
  return arr2d;
};

array2d.copy = function <T>(arr2d: Array2D<T>) {
  return array2d.from(arr2d, arr2d.nrows, arr2d.ncols);
};

array2d.subarray = function <T>(
  arr2d: Array2D<T>,
  top: number,
  left: number,
  nrows: number,
  ncols: number
) {
  const subarr2d = array2d<T>(nrows, ncols);
  for (let i = 0; i < nrows; ++i) {
    for (let j = 0; j < ncols; ++j) {
      subarr2d[i * ncols + j] = arr2d[(top + i) * arr2d.ncols + (left + j)];
    }
  }
  return subarr2d;
};

array2d.transpose = function <T>(arr2d: Array2D<T>) {
  const transposed = array2d<T>(arr2d.ncols, arr2d.nrows);
  for (let i = 0; i < arr2d.nrows; ++i) {
    for (let j = 0; j < arr2d.ncols; ++j) {
      transposed[j * transposed.ncols + i] = arr2d[i * arr2d.ncols + j];
    }
  }
  return transposed;
};

array2d.flipRowsInplace = function <T>(arr2d: Array2D<T>) {
  for (let i = 0; i < arr2d.nrows; ++i) {
    const limit = Math.floor(arr2d.ncols / 2);
    for (let j = 0; j < limit; ++j) {
      const tmp = arr2d[i * arr2d.ncols + j];
      arr2d[i * arr2d.ncols + j] =
        arr2d[i * arr2d.ncols + (arr2d.ncols - j) - 1];
      arr2d[i * arr2d.ncols + (arr2d.ncols - j) - 1] = tmp;
    }
  }
};

array2d.rot90 = function <T>(arr2d: Array2D<T>) {
  const transposed = array2d.transpose(arr2d);
  array2d.flipRowsInplace(transposed);
  return transposed;
};

array2d.equal = function <T>(l: Array2D<T>, r: Array2D<T>) {
  return (
    l.nrows == r.nrows && l.ncols == r.ncols && l.every((v, i) => v == r[i])
  );
};

array2d.pretty = function <T>(arr2d: Array2D<T>) {
  const buf = [];
  for (let i = 0; i < arr2d.nrows; ++i) {
    for (let j = 0; j < arr2d.ncols; ++j) {
      buf.push(arr2d[i * arr2d.ncols + j]);
      buf.push("\t");
    }
    buf.push("\n");
  }
  return buf.join("");
};

array2d.row = function <T>(arr2d: Array2D<T>, i: number) {
  return arr2d.slice(i * arr2d.ncols, (i + 1) * arr2d.ncols);
};

array2d.col = function <T>(arr2d: Array2D<T>, j: number) {
  const col = new Array<T>(arr2d.nrows);
  for (let i = 0; i < arr2d.nrows; ++i) {
    col[i] = arr2d[i * arr2d.ncols + j];
  }
  return col;
};
