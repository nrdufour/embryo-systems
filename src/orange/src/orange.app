{application, orange,
    [{description, "Orange"},
     {vsn, "0.1"},
     {modules, [orange, orange_sup,
            orange_class, orange_attribute, orange_link, orange_object,
            orange_storage_server]},
     {registered, [orange_class, orange_attribute, orange_link, orange_object,
               orange_storage_server, orange_sup]},
     {applications, [kernel,stdlib]},
     {mod, {orange, []}},
     {start_phases, []}
]}.

