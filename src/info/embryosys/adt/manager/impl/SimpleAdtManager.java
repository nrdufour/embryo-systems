/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.manager.impl;

import info.embryosys.adt.core.Adt;
import info.embryosys.adt.core.AdtId;
import info.embryosys.adt.core.AdtType;
import info.embryosys.adt.manager.AdtManager;
import info.embryosys.adt.manager.Workspace;
import info.embryosys.adt.request.AdtRequest;
import info.embryosys.adt.storage.Storage;
import info.embryosys.adt.storage.impl.DummyStorage;

import java.util.List;
import java.util.UUID;

/**
 * @author nrdufour
 * 
 */
public class SimpleAdtManager implements AdtManager {

	private Workspace workspace;

	/**
	 * @return the workspace
	 */
	public Workspace getWorkspace() {
		return workspace;
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.AdtManager#init()
	 */
	public void init() {
		// FIXME for now it's hardcoded .. I will find a way to configure that
		// through a conf file

		this.workspace = new Workspace("default", new DummyStorage());
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see
	 * info.embryosys.adt.manager.AdtManager#processRequest(info.embryosys.adt
	 * .manager.AdtRequest)
	 */
	public void processRequest(final AdtRequest request) {
		System.out.println("Processing request: " + request);

		switch (request.getOperation()) {
		case CREATE:
			create(request.getType(), request.getArguments());
			break;
		default:
		}
	}

	public void create(AdtType type, List<String> arguments) {
		Storage storage = workspace.getStorage();

		// FIXME need an ADT Factory here ...
		Adt adt = new Adt();
		AdtId id = new AdtId(type, UUID.randomUUID());
		adt.setId(id);

		// FIXME must be a better way to get that.
		// FIXME doesn't work for LINK !!!
		adt.setName(arguments.get(arguments.size() - 1));

		storage.store(adt);
	}
}
