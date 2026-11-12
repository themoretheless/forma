//! Reused command and attachment vectors.

use super::*;

impl MetalDevice {
    #[inline]
    pub(crate) fn take_cmd_ops(&self) -> Vec<crate::RecordedOp> {
        self.cmd_ops_pool
            .lock()
            .ok()
            .and_then(|mut p| p.pop())
            .unwrap_or_else(|| Vec::with_capacity(16))
    }

    #[inline]
    pub(crate) fn recycle_cmd_ops(&self, mut v: Vec<crate::RecordedOp>) {
        v.clear();
        if v.capacity() >= 4 {
            if let Ok(mut p) = self.cmd_ops_pool.lock() {
                if p.len() < 16 {
                    p.push(v);
                }
            }
        }
    }

    #[inline]
    pub(crate) fn take_render_cmds(&self) -> Vec<crate::RenderCommand> {
        self.render_cmds_pool
            .lock()
            .ok()
            .and_then(|mut p| p.pop())
            .unwrap_or_else(|| Vec::with_capacity(32))
    }

    #[inline]
    pub(crate) fn recycle_render_cmds(&self, mut v: Vec<crate::RenderCommand>) {
        v.clear();
        if v.capacity() >= 4 {
            if let Ok(mut p) = self.render_cmds_pool.lock() {
                if p.len() < 16 {
                    p.push(v);
                }
            }
        }
    }

    #[inline]
    pub(crate) fn take_color_atts(&self) -> Vec<Option<crate::OwnedColorAttachment>> {
        self.color_atts_pool
            .lock()
            .ok()
            .and_then(|mut p| p.pop())
            .unwrap_or_else(|| Vec::with_capacity(4))
    }

    #[inline]
    pub(crate) fn recycle_color_atts(&self, mut v: Vec<Option<crate::OwnedColorAttachment>>) {
        v.clear();
        if v.capacity() >= 1 {
            if let Ok(mut p) = self.color_atts_pool.lock() {
                if p.len() < 16 {
                    p.push(v);
                }
            }
        }
    }

    #[inline]
    pub(crate) fn take_compute_dispatches(&self) -> Vec<crate::ComputeDispatchOp> {
        self.compute_dispatches_pool
            .lock()
            .ok()
            .and_then(|mut p| p.pop())
            .unwrap_or_else(|| Vec::with_capacity(8))
    }

    #[inline]
    pub(crate) fn recycle_compute_dispatches(&self, mut v: Vec<crate::ComputeDispatchOp>) {
        v.clear();
        if v.capacity() >= 1 {
            if let Ok(mut p) = self.compute_dispatches_pool.lock() {
                if p.len() < 16 {
                    p.push(v);
                }
            }
        }
    }
}
