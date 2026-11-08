return {
    properties = { speed = 1.0 },
    on_start = function(self)
        fabr.log("spin started on " .. self.name)
    end,
    on_update = function(self, dt)
        self:rotate_y(self.properties.speed * dt)
    end,
}
