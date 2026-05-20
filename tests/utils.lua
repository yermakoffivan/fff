local M = {}

-- Normalize paths for windows
--- @param p string
--- @return string
function M.normalize(p)
  local rp = vim.uv.fs_realpath(p) or vim.fn.fnamemodify(vim.fn.resolve(p), ':p')
  local n = vim.fs.normalize(rp)
  n = n:gsub('/$', '')
  if vim.fn.has('win32') == 1 then n = n:lower() end
  return n
end

return M
