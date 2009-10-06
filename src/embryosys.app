%% This is the application resource file (.app file) for the 'base'
%% application.
{application, embryosys,
	[{description, "Embryo Systems"},
	 {vsn, "0.1"},
	 {modules, [embryosys_app, embryosys_sup, adt_server,
		    adtm_family, adtm_property, adtm_relation, adtm_entity,
		    storage_server]},
	 {registered, [adt_server, storage_server, embryosys_sup]},
	 {applications, [kernel,stdlib]},
	 {mod, {embryosys_app, []}},
	 {start_phases, []}
]}.

